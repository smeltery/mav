use action_log::ActionLog;
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result, anyhow};
use futures::FutureExt as _;
use gpui::{App, Entity, SharedString, Task};
use indoc::formatdoc;
use language::Point;
use language_model::{LanguageModelImage, LanguageModelImageExt, LanguageModelToolResultContent};
use project::{AgentLocation, ImageItem, Project, WorktreeSettings, image_store};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::path::Path;
use std::sync::Arc;
use util::markdown::MarkdownCodeBlock;

pub(super) use crate::ToolCallEventStream;
use crate::tools::tool_permissions::{
    ResolvedProjectPath, authorize_symlink_access, canonicalize_worktree_roots,
    resolve_global_skill_path, resolve_project_path,
};
use crate::{AgentTool, ToolInput, outline};

mod formatting;
mod global_skill;
#[cfg(test)]
mod test;

use formatting::{
    format_with_line_numbers, resolve_line_range, tool_content_err, write_lines_numbered,
};
use global_skill::read_global_skill_file;
/// The only supported path outside the project is `~/.agents/skills` or a descendant, for global agent skills.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileToolInput {
    /// The relative path of the file to read.
    ///
    /// This path should never be absolute, and the first component of the path should always be a root directory in a project, unless it's a global agent skill under `~/.agents/skills`.
    ///
    /// <example>
    /// If the project has the following root directories:
    ///
    /// - /a/b/directory1
    /// - /c/d/directory2
    ///
    /// If you want to access `file.txt` in `directory1`, you should use the path `directory1/file.txt`.
    /// If you want to access `file.txt` in `directory2`, you should use the path `directory2/file.txt`.
    /// </example>
    ///
    /// <example>
    /// To read a global agent skill file, you may provide a path under `~/.agents/skills`, such as `~/.agents/skills/my-skill/SKILL.md`.
    /// </example>
    pub path: String,
    /// Optional line number to start reading on (1-based index)
    #[serde(default)]
    pub start_line: Option<u32>,
    /// Optional line number to end reading on (1-based index, inclusive)
    #[serde(default)]
    pub end_line: Option<u32>,
}

pub struct ReadFileTool {
    project: Entity<Project>,
    action_log: Entity<ActionLog>,
    update_agent_location: bool,
}

impl ReadFileTool {
    pub fn new(
        project: Entity<Project>,
        action_log: Entity<ActionLog>,
        update_agent_location: bool,
    ) -> Self {
        Self {
            project,
            action_log,
            update_agent_location,
        }
    }
}

impl AgentTool for ReadFileTool {
    type Input = ReadFileToolInput;
    type Output = LanguageModelToolResultContent;

    const NAME: &'static str = "read_file";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Read
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input
            && let Some(project_path) = self.project.read(cx).find_project_path(&input.path, cx)
            && let Some(path) = self
                .project
                .read(cx)
                .short_full_path_for_project_path(&project_path, cx)
        {
            match (input.start_line, input.end_line) {
                (Some(start), Some(end)) => {
                    format!("Read file `{path}` (lines {}-{})", start, end,)
                }
                (Some(start), None) => {
                    format!("Read file `{path}` (from line {})", start)
                }
                _ => format!("Read file `{path}`"),
            }
            .into()
        } else {
            "Read file".into()
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<LanguageModelToolResultContent, LanguageModelToolResultContent>> {
        let project = self.project.clone();
        let action_log = self.action_log.clone();
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(tool_content_err)?;
            let fs = project.read_with(cx, |project, _cx| project.fs().clone());

            // Fast path: if the model passes a path that resolves under the
            // global skills directory, read it directly via the
            // filesystem. Global skills live outside any worktree, so the
            // standard project-path machinery would refuse them.
            if let Some(skill_path) =
                resolve_global_skill_path(Path::new(&input.path), fs.as_ref()).await
            {
                return read_global_skill_file(
                    &skill_path,
                    fs.as_ref(),
                    input.start_line,
                    input.end_line,
                    &input.path,
                    &event_stream,
                )
                .await;
            }

            let canonical_roots = canonicalize_worktree_roots(&project, &fs, cx).await;

            let (project_path, symlink_canonical_target) =
                project.read_with(cx, |project, cx| {
                    let resolved =
                        resolve_project_path(project, &input.path, &canonical_roots, cx)?;
                    anyhow::Ok(match resolved {
                        ResolvedProjectPath::Safe(path) => (path, None),
                        ResolvedProjectPath::SymlinkEscape {
                            project_path,
                            canonical_target,
                        } => (project_path, Some(canonical_target)),
                    })
                }).map_err(tool_content_err)?;

            let abs_path = project
                .read_with(cx, |project, cx| {
                    project.absolute_path(&project_path, cx)
                })
                .ok_or_else(|| {
                    anyhow!("Failed to convert {} to absolute path", &input.path)
                }).map_err(tool_content_err)?;

            // Check settings exclusions synchronously
            project.read_with(cx, |_project, cx| {
                let global_settings = WorktreeSettings::get_global(cx);
                if global_settings.is_path_excluded(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the global `file_scan_exclusions` setting: {}",
                        &input.path
                    );
                }

                if global_settings.is_path_private(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the global `private_files` setting: {}",
                        &input.path
                    );
                }

                let worktree_settings = WorktreeSettings::get(Some((&project_path).into()), cx);
                if worktree_settings.is_path_excluded(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the worktree `file_scan_exclusions` setting: {}",
                        &input.path
                    );
                }

                if worktree_settings.is_path_private(&project_path.path) {
                    anyhow::bail!(
                        "Cannot read file because its path matches the worktree `private_files` setting: {}",
                        &input.path
                    );
                }

                anyhow::Ok(())
            }).map_err(tool_content_err)?;

            if fs.is_dir(&abs_path).await {
                return Err(tool_content_err(format!(
                    "{} is a directory, not a file. Use the list_directory tool to explore directory contents.",
                    &input.path
                )));
            }

            if let Some(canonical_target) = &symlink_canonical_target {
                let authorize = cx.update(|cx| {
                    authorize_symlink_access(
                        Self::NAME,
                        &input.path,
                        canonical_target,
                        &event_stream,
                        cx,
                    )
                });
                authorize.await.map_err(tool_content_err)?;
            }

            let file_path = input.path.clone();

            cx.update(|_cx| {
                event_stream.update_fields(acp::ToolCallUpdateFields::new().locations(vec![
                    acp::ToolCallLocation::new(&abs_path)
                        .line(input.start_line.map(|line| line.saturating_sub(1))),
                ]));
            });

            let is_image = project.read_with(cx, |_project, cx| {
                image_store::is_image_file(&project, &project_path, cx)
            });

            if is_image {
                let image_entity: Entity<ImageItem> = cx
                    .update(|cx| {
                        self.project.update(cx, |project, cx| {
                            project.open_image(project_path.clone(), cx)
                        })
                    })
                    .await.map_err(tool_content_err)?;

                let image =
                    image_entity.read_with(cx, |image_item, _| Arc::clone(&image_item.image));

                let language_model_image = cx
                    .update(|cx| LanguageModelImage::from_image(image, cx))
                    .await
                    .context("processing image")
                    .map_err(tool_content_err)?;

                event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                    acp::ToolCallContent::Content(acp::Content::new(acp::ContentBlock::Image(
                        acp::ImageContent::new(language_model_image.source.clone(), "image/png"),
                    ))),
                ]));

                return Ok(language_model_image.into());
            }

            let open_buffer_task = project.update(cx, |project, cx| {
                project.open_buffer(project_path.clone(), cx)
            });

            let buffer = futures::select! {
                result = open_buffer_task.fuse() => result.map_err(tool_content_err)?,
                _ = event_stream.cancelled_by_user().fuse() => {
                    return Err(tool_content_err("File read cancelled by user"));
                }
            };
            if buffer.read_with(cx, |buffer, _| {
                buffer
                    .file()
                    .as_ref()
                    .is_none_or(|file| !file.disk_state().exists())
            }) {
                return Err(tool_content_err(format!("{file_path} not found")));
            }

            let mut anchor = None;
            let mut is_outline_response = false;

            // Check if specific line ranges are provided
            let result = if input.start_line.is_some() || input.end_line.is_some() {
                let result_text = buffer.read_with(cx, |buffer, _cx| {
                    let (start, end) = resolve_line_range(input.start_line, input.end_line);
                    let start_row = start - 1;
                    if start_row <= buffer.max_point().row {
                        let column = buffer.line_indent_for_row(start_row).raw_len();
                        anchor = Some(buffer.anchor_before(Point::new(start_row, column)));
                    }

                    // `end` is 1-indexed inclusive; `Point` rows are 0-indexed.
                    // Using `end` directly as the (exclusive) end row is the
                    // standard inclusive→exclusive translation, and since
                    // `resolve_line_range` guarantees `end >= start`, we always
                    // read at least one line.
                    let start_anchor = buffer.anchor_before(Point::new(start_row, 0));
                    let end_anchor = buffer.anchor_before(Point::new(end, 0));
                    // Stream the numbered output directly from the buffer's
                    // chunk iterator so the unnumbered range is never
                    // materialized as its own `String`.
                    let mut output = String::new();
                    write_lines_numbered(
                        &mut output,
                        buffer.text_for_range(start_anchor..end_anchor),
                        start,
                    );
                    output
                });

                action_log.update(cx, |log, cx| {
                    log.buffer_read(buffer.clone(), cx);
                });

                Ok(result_text.into())
            } else {
                // No line ranges specified, so check file size to see if it's too big.
                let buffer_content = outline::get_buffer_content_or_outline(
                    buffer.clone(),
                    Some(&abs_path.to_string_lossy()),
                    cx,
                )
                .await.map_err(tool_content_err)?;

                action_log.update(cx, |log, cx| {
                    log.buffer_read(buffer.clone(), cx);
                });


                is_outline_response = buffer_content.is_synthetic;

                if buffer_content.is_synthetic {
                    Ok(formatdoc! {"
                        SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

                        IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
                        Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

                        {}

                        NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
                        For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.", buffer_content.text
                    }
                    .into())
                } else {
                    Ok(format_with_line_numbers(&buffer_content.text, 1).into())
                }
            };

            project.update(cx, |project, cx| {
                if self.update_agent_location {
                    project.set_agent_location(
                        Some(AgentLocation {
                            buffer: buffer.downgrade(),
                            position: anchor.unwrap_or_else(|| {
                                text::Anchor::min_for_buffer(buffer.read(cx).remote_id())
                            }),
                        }),
                        cx,
                    );
                }
                if let Ok(LanguageModelToolResultContent::Text(text)) = &result {
                    let text: &str = text;
                    // For outline responses, omit the path tag so the markdown renderer
                    // does not invoke tree-sitter syntax highlighting against pseudo-code
                    // outline text. The outline is not valid source for the file's language,
                    // so highlighting would be both expensive and incorrect.
                    let tag: &str = if is_outline_response { "" } else { &input.path };
                    let markdown = MarkdownCodeBlock { tag, text }.to_string();
                    event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                        acp::ToolCallContent::Content(acp::Content::new(markdown)),
                    ]));
                }
            });

            result
        })
    }

    fn replay(
        &self,
        input: Self::Input,
        output: Self::Output,
        event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Result<()> {
        if let LanguageModelToolResultContent::Text(text) = output {
            let markdown = MarkdownCodeBlock {
                tag: &input.path,
                text: &text,
            }
            .to_string();
            event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                acp::ToolCallContent::Content(acp::Content::new(markdown)),
            ]));
        }

        Ok(())
    }
}
