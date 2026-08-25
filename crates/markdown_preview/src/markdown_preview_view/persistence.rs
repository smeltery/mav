use super::*;

use std::path::PathBuf;

use db::{
    query,
    sqlez::{domain::Domain, thread_safe_connection::ThreadSafeConnection},
    sqlez_macros::sql,
};
use workspace::{ItemId, WorkspaceDb, WorkspaceId};

pub struct MarkdownPreviewDb(ThreadSafeConnection);

impl Domain for MarkdownPreviewDb {
    const NAME: &str = stringify!(MarkdownPreviewDb);

    const MIGRATIONS: &[&str] = &[sql!(
        CREATE TABLE markdown_previews (
            workspace_id INTEGER,
            item_id INTEGER,
            abs_path BLOB,
            mode INTEGER NOT NULL DEFAULT 0,

            PRIMARY KEY(workspace_id, item_id),
            FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
            ON DELETE CASCADE
        ) STRICT;
    )];
}

db::static_connection!(MarkdownPreviewDb, [WorkspaceDb]);

impl MarkdownPreviewDb {
    query! {
        pub async fn save_preview(
            item_id: ItemId,
            workspace_id: WorkspaceId,
            abs_path: PathBuf,
            mode: i64
        ) -> Result<()> {
            INSERT OR REPLACE INTO markdown_previews(item_id, workspace_id, abs_path, mode)
            VALUES (?, ?, ?, ?)
        }
    }

    query! {
        pub fn get_preview(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<(PathBuf, i64)>> {
            SELECT abs_path, mode
            FROM markdown_previews
            WHERE item_id = ? AND workspace_id = ?
        }
    }
}
