use std::any::TypeId;
use std::borrow::Cow;
use std::cmp::min;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use editor::scroll::Autoscroll;
use editor::{Editor, EditorEvent, MultiBufferOffset, SelectionEffects};
use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, ImageSource,
    InteractiveElement, IntoElement, IsZero, Pixels, Render, Resource, RetainAllImageCache,
    ScrollHandle, SharedString, SharedUri, Subscription, Task, WeakEntity, Window, point, px,
};
use language::LanguageRegistry;
use markdown::{
    CodeBlockRenderer, CopyButtonVisibility, Markdown, MarkdownElement, MarkdownFont,
    MarkdownOptions, MarkdownStyle,
};
use mav_actions::{DecreaseBufferFontSize, IncreaseBufferFontSize, ResetBufferFontSize};
use project::search::SearchQuery;
use project::{Project, ProjectPath};
use settings::{SeedQuerySetting, Settings, update_settings_file};
use theme::{SystemAppearance, Theme, ThemeRegistry};
use theme_settings::ThemeSettings;
use ui::utils::WithRemSize;
use ui::{ContextMenu, WithScrollbar, prelude::*, right_click_menu};
use util::markdown::split_local_url_fragment;
use workspace::item::{Item, ItemBufferKind, ItemHandle, SaveOptions, SerializableItem};
use workspace::notifications::NotifyResultExt;
use workspace::searchable::{
    Direction, SearchEvent, SearchOptions, SearchToken, SearchableItem, SearchableItemHandle,
};
use workspace::{ItemId, Pane, Workspace, WorkspaceId, delete_unloaded_items};

use crate::markdown_preview_settings::MarkdownPreviewSettings;
use crate::{
    OpenFollowingPreview, OpenPreview, OpenPreviewToTheSide, ScrollDown, ScrollDownByItem,
};
use crate::{ScrollPageDown, ScrollPageUp, ScrollToBottom, ScrollToTop, ScrollUp, ScrollUpByItem};

const REPARSE_DEBOUNCE: Duration = Duration::from_millis(200);

pub struct MarkdownPreviewView {
    workspace: WeakEntity<Workspace>,
    active_editor: Option<EditorState>,
    focus_handle: FocusHandle,
    markdown: Entity<Markdown>,
    _markdown_subscription: Subscription,
    active_source_index: Option<usize>,
    scroll_handle: ScrollHandle,
    image_cache: Entity<RetainAllImageCache>,
    base_directory: Option<PathBuf>,
    pending_update_task: Option<Task<Result<()>>>,
    mode: MarkdownPreviewMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkdownPreviewMode {
    /// The preview will always show the contents of the provided editor.
    Default,
    /// The preview will "follow" the currently active editor.
    Follow,
}

impl MarkdownPreviewMode {
    fn to_db(self) -> i64 {
        match self {
            Self::Default => 0,
            Self::Follow => 1,
        }
    }

    fn from_db(value: i64) -> Self {
        match value {
            1 => Self::Follow,
            _ => Self::Default,
        }
    }
}

struct EditorState {
    editor: Entity<Editor>,
    _subscription: Subscription,
}

#[derive(Clone, Copy, Debug)]
pub enum MarkdownPreviewEvent {
    SourceEditorChanged,
    SourceFileHandleChanged,
}

#[path = "markdown_preview_view/actions.rs"]
mod actions;
#[path = "markdown_preview_view/editor_sync.rs"]
mod editor_sync;
#[path = "markdown_preview_view/item.rs"]
mod item;
#[path = "markdown_preview_view/persistence.rs"]
mod persistence;
#[path = "markdown_preview_view/registration.rs"]
mod registration;
#[path = "markdown_preview_view/render.rs"]
mod render;
#[path = "markdown_preview_view/render_helpers.rs"]
mod render_helpers;
#[path = "markdown_preview_view/search.rs"]
mod search;
#[path = "markdown_preview_view/serialization.rs"]
mod serialization;
#[cfg(test)]
#[path = "markdown_preview_view/tests/mod.rs"]
mod tests;
#[path = "markdown_preview_view/url.rs"]
mod url;

pub(crate) use url::{handle_url_click, resolve_preview_image};
