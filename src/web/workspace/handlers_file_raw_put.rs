//! `PUT /workspace/file/raw`：把请求体原样写入工作区（文本与二进制；上限与 JSON 写入相同）。

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;

use super::handlers::effective_workspace_base_canonical;
#[cfg(unix)]
use super::handlers_sync::workspace_file_write_sync_unix;
use super::handlers_file_raw::{raw_err, reject_unsafe_rel_path, RawErr};
use crate::web::app_state::AppStateHttpCore;
use crate::web::http_types::validation::validate_workspace_file_write_payload;
use crate::web::http_types::workspace::WorkspaceFileRawPutQuery;
use crate::workspace::path::resolve_web_workspace_write_path;

/// 写入工作区原始字节；成功 **204**。失败为 JSON `ApiError`（与 GET raw 同类）。
pub async fn workspace_file_raw_put_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceFileRawPutQuery>,
    body: Bytes,
) -> Result<StatusCode, RawErr> {
    if query.create_only && query.update_only {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_FLAGS_CONFLICT",
            "create_only 与 update_only 不能同时为 true",
        ));
    }
    let path = query.path.trim();
    reject_unsafe_rel_path(path)?;
    if let Err(e) = validate_workspace_file_write_payload(body.as_ref()) {
        return Err(raw_err(StatusCode::PAYLOAD_TOO_LARGE, "WORKSPACE_FILE_TOO_LARGE", e));
    }
    let base = effective_workspace_base_canonical(&http)
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_UNAVAILABLE",
                e.user_message(),
            )
        })?;
    let canonical = resolve_web_workspace_write_path(&base, path).map_err(|e| {
        let status = if e.is_policy_denied() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        raw_err(status, "WORKSPACE_PATH_DENIED", e.user_message())
    })?;
    write_raw_bytes(
        base,
        canonical,
        body.to_vec(),
        query.create_only,
        query.update_only,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn write_raw_bytes(
    base: std::path::PathBuf,
    canonical: std::path::PathBuf,
    content: Vec<u8>,
    create_only: bool,
    update_only: bool,
) -> Result<(), RawErr> {
    #[cfg(unix)]
    {
        tokio::task::spawn_blocking(move || {
            workspace_file_write_sync_unix(base, canonical, content, create_only, update_only)
        })
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "WORKSPACE_FILE_WRITE",
                format!("写入文件任务失败: {e}"),
            )
        })?
        .map_err(map_write_msg)
    }
    #[cfg(not(unix))]
    {
        let _ = base;
        write_raw_bytes_non_unix(canonical, content, create_only, update_only).await
    }
}

fn map_write_msg(msg: String) -> RawErr {
    if msg.contains("已存在") {
        return raw_err(StatusCode::CONFLICT, "WORKSPACE_FILE_EXISTS", msg);
    }
    if msg.contains("不存在") {
        return raw_err(StatusCode::NOT_FOUND, "WORKSPACE_FILE_MISSING", msg);
    }
    raw_err(StatusCode::BAD_REQUEST, "WORKSPACE_FILE_WRITE", msg)
}

#[cfg(not(unix))]
async fn write_raw_bytes_non_unix(
    canonical: std::path::PathBuf,
    content: Vec<u8>,
    create_only: bool,
    update_only: bool,
) -> Result<(), RawErr> {
    let exists = tokio::fs::try_exists(&canonical).await.unwrap_or(false);
    if create_only && exists {
        return Err(raw_err(
            StatusCode::CONFLICT,
            "WORKSPACE_FILE_EXISTS",
            "文件已存在，无法仅创建",
        ));
    }
    if update_only && !exists {
        return Err(raw_err(
            StatusCode::NOT_FOUND,
            "WORKSPACE_FILE_MISSING",
            "文件不存在，无法仅修改",
        ));
    }
    if let Some(parent) = canonical.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_FILE_WRITE",
            format!("创建目录失败: {e}"),
        ));
    }
    tokio::fs::write(&canonical, content).await.map_err(|e| {
        raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_FILE_WRITE",
            format!("写入文件失败: {e}"),
        )
    })
}
