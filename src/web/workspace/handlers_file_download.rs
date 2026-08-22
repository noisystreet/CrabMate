//! `GET /workspace/file/download`：工作区任意文件原始字节（供 Client「保存到本机」；非聊天内嵌图）。

use axum::extract::{Query, State};
use axum::response::Response;

use super::handlers_file_raw::{
    bytes_ok_response, load_workspace_rel_file_bytes, reject_unsafe_rel_path, RawErr,
};
use crate::cm_web_host::http_types::limits::WORKSPACE_FILE_WRITE_MAX_BYTES;
use crate::web::app_state::AppStateHttpCore;
use crate::web::http_types::workspace::WorkspaceFileQuery;

/// 读取工作区文件原始字节；失败为 JSON `ApiError`。上限与 `PUT /workspace/file/raw` 相同。
pub async fn workspace_file_download_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Result<Response, RawErr> {
    let path = query.path.trim();
    reject_unsafe_rel_path(path)?;
    let bytes = load_workspace_rel_file_bytes(
        &http,
        path,
        WORKSPACE_FILE_WRITE_MAX_BYTES as u64,
        "WORKSPACE_FILE_READ",
    )
    .await?;
    bytes_ok_response(
        bytes,
        "application/octet-stream",
        "private, no-store",
        "WORKSPACE_FILE_RESPONSE",
    )
}
