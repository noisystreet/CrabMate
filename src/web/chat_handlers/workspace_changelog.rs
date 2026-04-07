//! `GET /workspace/changelog`。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};

use super::super::app_state::AppState;
use super::parse::normalize_client_conversation_id;
use super::types::{WorkspaceChangelogQuery, WorkspaceChangelogResponse};
use crate::workspace_changelist;

pub(crate) async fn workspace_changelog_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<WorkspaceChangelogQuery>,
) -> Json<WorkspaceChangelogResponse> {
    let cid = match normalize_client_conversation_id(q.conversation_id.as_deref()) {
        Ok(o) => o,
        Err(msg) => {
            return Json(WorkspaceChangelogResponse {
                revision: 0,
                markdown: String::new(),
                error: Some(msg),
            });
        }
    };
    let scope = cid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__default__");
    let cfg = state.cfg.read().await;
    if !cfg.session_workspace_changelist_enabled {
        return Json(WorkspaceChangelogResponse {
            revision: 0,
            markdown: String::new(),
            error: Some(
                "会话工作区变更集已在配置中关闭（session_workspace_changelist_enabled）"
                    .to_string(),
            ),
        });
    }
    let max_chars = cfg.session_workspace_changelist_max_chars;
    drop(cfg);
    let cl = workspace_changelist::changelist_for_scope(scope);
    let (rev, body) = cl.snapshot_markdown(max_chars);
    Json(WorkspaceChangelogResponse {
        revision: rev,
        markdown: body.unwrap_or_default(),
        error: None,
    })
}
