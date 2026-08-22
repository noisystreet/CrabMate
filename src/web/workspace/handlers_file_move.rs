//! `POST /workspace/file/move`：工作区内重命名/移动常规文件（与工具 `move_file` 同策略）。

use axum::extract::{Json, State};
use axum::http::StatusCode;

use super::handlers::effective_workspace_base_canonical;
use super::handlers_file_raw::{
    map_workspace_rel_read_err, raw_err, reject_unsafe_rel_path, RawErr,
};
use crate::web::app_state_facets::WebChangelogAppFacet;
use crate::web::http_types::workspace::WorkspaceFileMoveBody;
use crate::web::normalize_client_conversation_id;
use crate::workspace::changelist::record_file_state_after_write;
use crate::workspace::fs::rename_file_under_root;
use crate::workspace::path::{resolve_web_workspace_read_path, resolve_web_workspace_write_path};

/// 移动成功 **204**；失败 JSON `ApiError`。
pub async fn workspace_file_move_handler(
    State(facet): State<WebChangelogAppFacet>,
    Json(body): Json<WorkspaceFileMoveBody>,
) -> Result<StatusCode, RawErr> {
    let (from, to) = parse_move_rels(&body)?;
    let cid = normalize_client_conversation_id(body.conversation_id.as_deref()).map_err(|msg| {
        raw_err(StatusCode::BAD_REQUEST, "INVALID_CONVERSATION_ID", msg)
    })?;
    let base = effective_workspace_base_canonical(&facet.http)
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_UNAVAILABLE",
                e.user_message(),
            )
        })?;
    let (src, dst) = resolve_move_endpoints(&base, from, to, body.overwrite).await?;
    spawn_rename(base.clone(), src, dst).await?;
    record_move_changelist(&facet, &base, from, to, cid.as_deref()).await;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_move_rels(body: &WorkspaceFileMoveBody) -> Result<(&str, &str), RawErr> {
    let from = body.from.trim();
    let to = body.to.trim();
    reject_unsafe_rel_path(from)?;
    reject_unsafe_rel_path(to)?;
    if from == to {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_MOVE_SAME_PATH",
            "from 与 to 相同",
        ));
    }
    Ok((from, to))
}

async fn resolve_move_endpoints(
    base: &std::path::Path,
    from: &str,
    to: &str,
    overwrite: bool,
) -> Result<(std::path::PathBuf, std::path::PathBuf), RawErr> {
    let src = resolve_web_workspace_read_path(base, Some(from))
        .map_err(|e| map_workspace_rel_read_err(e, "WORKSPACE_FILE_MISSING"))?;
    ensure_regular_file(&src).await?;
    let dst = resolve_web_workspace_write_path(base, to).map_err(map_write_path_err)?;
    check_move_dest(&dst, overwrite).await?;
    Ok((src, dst))
}

async fn spawn_rename(
    base: std::path::PathBuf,
    src: std::path::PathBuf,
    dst: std::path::PathBuf,
) -> Result<(), RawErr> {
    tokio::task::spawn_blocking(move || rename_file_under_root(&base, &src, &dst))
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "WORKSPACE_FILE_MOVE",
                format!("移动任务失败: {e}"),
            )
        })?
        .map_err(|e| {
            raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_FILE_MOVE",
                format!("移动失败: {e}"),
            )
        })
}

fn map_write_path_err(e: crate::workspace::path::WorkspacePathError) -> RawErr {
    let status = if e.is_policy_denied() {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::BAD_REQUEST
    };
    raw_err(status, "WORKSPACE_PATH_DENIED", e.user_message())
}

async fn ensure_regular_file(src: &std::path::Path) -> Result<(), RawErr> {
    let meta = tokio::fs::metadata(src).await.map_err(|_| {
        raw_err(
            StatusCode::NOT_FOUND,
            "WORKSPACE_FILE_MISSING",
            "源文件不存在",
        )
    })?;
    if !meta.is_file() {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_NOT_A_FILE",
            "仅支持移动常规文件；目录请用打包下载",
        ));
    }
    Ok(())
}

async fn check_move_dest(dst: &std::path::Path, overwrite: bool) -> Result<(), RawErr> {
    let Ok(meta) = tokio::fs::metadata(dst).await else {
        return Ok(());
    };
    if meta.is_dir() {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_DEST_IS_DIR",
            "目标是已存在的目录，请指定文件路径",
        ));
    }
    if meta.is_file() && !overwrite {
        return Err(raw_err(
            StatusCode::CONFLICT,
            "WORKSPACE_FILE_EXISTS",
            "目标文件已存在；覆盖请设 overwrite 为 true",
        ));
    }
    Ok(())
}

async fn record_move_changelist(
    facet: &WebChangelogAppFacet,
    working_dir: &std::path::Path,
    from: &str,
    to: &str,
    conversation_id: Option<&str>,
) {
    let cfg = facet.http.cfg.read().await;
    if !cfg
        .session_workspace_changelist
        .session_workspace_changelist_enabled
    {
        return;
    }
    drop(cfg);
    let scope = conversation_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__default__");
    let cl = facet
        .process_handles
        .workspace_changelist_registry
        .changelist_for_scope(scope);
    record_file_state_after_write(Some(&cl), working_dir, from, None);
    record_file_state_after_write(Some(&cl), working_dir, to, None);
}
