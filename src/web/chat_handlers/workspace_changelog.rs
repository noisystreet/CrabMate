//! `GET /workspace/changelog`。

use axum::Json;
use axum::extract::{Query, State};

use super::parse::normalize_client_conversation_id;
use crate::web::app_state_facets::WebChangelogAppFacet;
use crate::web::http_types::workspace::{WorkspaceChangelogQuery, WorkspaceChangelogResponse};
pub(crate) async fn workspace_changelog_handler(
    State(facet): State<WebChangelogAppFacet>,
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
    let cfg = facet.http.cfg.read().await;
    if !cfg
        .session_workspace_changelist
        .session_workspace_changelist_enabled
    {
        return Json(WorkspaceChangelogResponse {
            revision: 0,
            markdown: String::new(),
            error: Some(
                "会话工作区变更集已在配置中关闭（session_workspace_changelist_enabled）"
                    .to_string(),
            ),
        });
    }
    let max_chars = cfg
        .session_workspace_changelist
        .session_workspace_changelist_max_chars;
    drop(cfg);
    let cl = facet
        .process_handles
        .workspace_changelist_registry
        .changelist_for_scope(scope);
    let (rev, body) = cl.snapshot_markdown(max_chars);
    Json(WorkspaceChangelogResponse {
        revision: rev,
        markdown: body.unwrap_or_default(),
        error: None,
    })
}
