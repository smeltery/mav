use super::{
    stack_frame_list::{StackFrameList, StackFrameListEvent},
    variable_list::VariableList,
};
use anyhow::Result;
use collections::HashMap;
use dap::{CompletionItem, CompletionItemType, OutputEvent};
use editor::{
    Bias, CompletionProvider, Editor, EditorElement, EditorMode, EditorStyle, HighlightKey,
    MultiBufferOffset, SizingBehavior,
};
use fuzzy::StringMatchCandidate;
use gpui::{
    Action as _, AppContext, Context, Entity, FocusHandle, Focusable, HighlightStyle, Hsla, Render,
    Subscription, Task, TextStyle, WeakEntity, actions,
};
use language::{Anchor, Buffer, CharScopeContext, CodeLabel, TextBufferSnapshot, ToOffset};
use menu::{Confirm, SelectNext, SelectPrevious};
use project::{
    CompletionDisplayOptions, CompletionResponse,
    debugger::session::{CompletionsQuery, OutputToken, Session},
    lsp_store::CompletionDocumentation,
    search_history::{SearchHistory, SearchHistoryCursor},
};
use settings::Settings;
use std::{ops::Range, rc::Rc, usize};
use theme::Theme;
use theme_settings::ThemeSettings;
use ui::{ContextMenu, Divider, PopoverMenu, SplitButton, Tooltip, prelude::*};
use util::ResultExt;

actions!(
    console,
    [
        /// Adds an expression to the watch list.
        WatchExpression
    ]
);

mod completion;
mod constructor;
mod output;
mod query;
mod root_render;
#[cfg(test)]
mod tests;

pub(super) use completion::ConsoleQueryBarCompletionProvider;

pub struct Console {
    console: Entity<Editor>,
    query_bar: Entity<Editor>,
    session: Entity<Session>,
    _subscriptions: Vec<Subscription>,
    variable_list: Entity<VariableList>,
    stack_frame_list: Entity<StackFrameList>,
    last_token: OutputToken,
    update_output_task: Option<Task<()>>,
    focus_handle: FocusHandle,
    history: SearchHistory,
    cursor: SearchHistoryCursor,
}

impl Focusable for Console {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
