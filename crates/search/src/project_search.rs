use crate::{
    BufferSearchBar, EXCLUDE_PLACEHOLDER, FocusSearch, HighlightKey, INCLUDE_PLACEHOLDER,
    NextHistoryQuery, PreviousHistoryQuery, REPLACE_PLACEHOLDER, ReplaceAll, ReplaceNext,
    SearchOption, SearchOptions, SearchSource, SelectNextMatch, SelectPreviousMatch,
    ToggleCaseSensitive, ToggleIncludeIgnored, ToggleRegex, ToggleReplace, ToggleWholeWord,
    buffer_search::Deploy,
    search_bar::{
        ActionButtonState, HistoryNavigationDirection, alignment_element, input_base_styles,
        render_action_button, render_text_input, should_navigate_history,
    },
    text_finder::TextFinder,
};
use anyhow::Context as _;
use collections::HashMap;
use editor::{
    Anchor, Editor, EditorEvent, EditorSettings, MAX_TAB_TITLE_LEN, MultiBuffer, PathKey,
    SelectionEffects,
    actions::{Backtab, FoldAll, SelectAll, Tab, UnfoldAll},
    items::active_match_index,
    multibuffer_context_lines,
    scroll::Autoscroll,
};
use futures::{StreamExt, stream::FuturesOrdered};
use gpui::{
    Action, AnyElement, App, AsyncApp, Axis, Context, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, Global, Hsla, InteractiveElement, IntoElement, KeyContext, ParentElement, Point,
    Render, SharedString, Styled, Subscription, Task, TaskExt, UpdateGlobal, WeakEntity, Window,
    actions, div,
};
use itertools::Itertools;
use language::{Buffer, Language};
use menu::Confirm;
use multi_buffer;
use project::{
    Project, ProjectPath, SearchResults,
    search::{SearchInputKind, SearchQuery, SearchResult},
    search_history::SearchHistoryCursor,
};
use settings::Settings;
use std::{
    any::{Any, TypeId},
    mem,
    ops::{Not, Range},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use ui::{
    CommonAnimationExt, IconButtonShape, KeyBinding, Toggleable, Tooltip, prelude::*,
    utils::SearchInputWidth,
};
use util::{ResultExt as _, paths::PathMatcher, rel_path::RelPath};
use workspace::{
    DeploySearch, ItemNavHistory, NewSearch, ToolbarItemEvent, ToolbarItemLocation,
    ToolbarItemView, Workspace, WorkspaceId,
    item::{Item, ItemEvent, ItemHandle, SaveOptions},
    searchable::{Direction, SearchEvent, SearchToken, SearchableItem, SearchableItemHandle},
};

mod glob;
mod model;
mod search_bar;
mod search_bar_render;
mod state;
mod view_core;
mod view_deploy;
mod view_item;
mod view_navigation;
mod view_query;
mod workspace_actions;

use glob::split_glob_patterns;
use state::{InputPanel, ProjectSearchSettings, SearchActivity, SearchCompletion, SearchState};
pub use state::{ProjectSearch, ProjectSearchBar, ProjectSearchView};

actions!(
    project_search,
    [
        /// Searches in a new project search tab.
        SearchInNew,
        /// Toggles focus between the search bar and the search results.
        ToggleFocus,
        /// Moves to the next input field.
        NextField,
        /// Toggles the search filters panel.
        ToggleFilters,
        /// Toggles collapse/expand state of all search result excerpts.
        ToggleAllSearchResults,
        /// Open a text picker showing the current result in a modal.
        OpenTextFinder
    ]
);

#[derive(Default)]
pub(crate) struct ActiveSettings(pub(crate) HashMap<WeakEntity<Project>, ProjectSearchSettings>);

impl Global for ActiveSettings {}

pub use workspace_actions::init;
pub(crate) use model::contains_uppercase;
pub(crate) use view_navigation::buffer_search_query;

#[cfg(test)]
mod tests;
