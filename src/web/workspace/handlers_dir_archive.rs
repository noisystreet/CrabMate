//! `GET /workspace/dir/archive`：工作区目录 zip（供 Client 保存整个文件夹）。

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;

use super::handlers::effective_workspace_base_canonical;
use super::handlers_file_raw::{bytes_ok_response, raw_err, reject_unsafe_rel_path, RawErr};
use crate::web::app_state::AppStateHttpCore;
use crate::web::http_types::workspace::WorkspaceDirArchiveQuery;
use crate::workspace::path::resolve_web_workspace_read_path;

/// 打包工作区目录为 zip；失败为 JSON `ApiError`。
pub async fn workspace_dir_archive_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceDirArchiveQuery>,
) -> Result<Response, RawErr> {
    #[cfg(not(feature = "archive-tools"))]
    {
        let _ = (http, query);
        return Err(raw_err(
            StatusCode::NOT_IMPLEMENTED,
            "WORKSPACE_ARCHIVE_UNAVAILABLE",
            "当前构建未启用 archive-tools，无法打包目录",
        ));
    }
    #[cfg(feature = "archive-tools")]
    {
        archive_dir_enabled(&http, query).await
    }
}

#[cfg(feature = "archive-tools")]
async fn archive_dir_enabled(
    http: &AppStateHttpCore,
    query: WorkspaceDirArchiveQuery,
) -> Result<Response, RawErr> {
    let rel = query.path.as_deref().unwrap_or("").trim();
    if !rel.is_empty() {
        reject_unsafe_rel_path(rel)?;
    }
    let (base, canonical) = resolve_archive_dir(http, rel).await?;
    let prefix = zip_prefix_for_rel(rel);
    let bytes = tokio::task::spawn_blocking(move || {
        super::handlers_dir_archive_zip::zip_directory_bytes(&base, &canonical, &prefix)
    })
    .await
    .map_err(|e| {
        raw_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WORKSPACE_ARCHIVE_WALK",
            format!("打包任务失败: {e}"),
        )
    })?
    .map_err(|e| {
        let status = if e.code == "WORKSPACE_ARCHIVE_TOO_LARGE" {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        };
        raw_err(status, e.code, e.message)
    })?;
    #[cfg(feature = "archive-tools")]
    zip_ok_response(
        bytes,
        &super::handlers_dir_archive_zip::archive_zip_filename(rel),
    )
}

#[cfg(feature = "archive-tools")]
fn zip_prefix_for_rel(rel: &str) -> String {
    super::handlers_dir_archive_zip::archive_dir_stem(rel).to_string()
}

#[cfg(feature = "archive-tools")]
async fn resolve_archive_dir(
    http: &AppStateHttpCore,
    rel: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), RawErr> {
    use super::handlers_file_raw::map_workspace_rel_read_err;
    let base = effective_workspace_base_canonical(http)
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_UNAVAILABLE",
                e.user_message(),
            )
        })?;
    let sub = if rel.is_empty() { None } else { Some(rel) };
    let canonical = resolve_web_workspace_read_path(&base, sub)
        .map_err(|e| map_workspace_rel_read_err(e, "WORKSPACE_DIR_MISSING"))?;
    let meta = tokio::fs::metadata(&canonical).await.map_err(|_| {
        raw_err(
            StatusCode::NOT_FOUND,
            "WORKSPACE_DIR_MISSING",
            "目录不存在",
        )
    })?;
    if !meta.is_dir() {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_NOT_A_DIRECTORY",
            "path 不是目录；单文件请用 GET /workspace/file/download",
        ));
    }
    Ok((base, canonical))
}

#[cfg(feature = "archive-tools")]
fn zip_ok_response(bytes: Vec<u8>, filename: &str) -> Result<Response, RawErr> {
    let mut resp = bytes_ok_response(
        bytes,
        "application/zip",
        "private, no-store",
        "WORKSPACE_ARCHIVE_RESPONSE",
    )?;
    let ascii_ok = filename
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    let disp = if ascii_ok {
        format!("attachment; filename=\"{filename}\"")
    } else {
        "attachment; filename=\"archive.zip\"".to_string()
    };
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        disp.parse().map_err(|e| {
            raw_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "WORKSPACE_ARCHIVE_RESPONSE",
                format!("构造 Content-Disposition 失败: {e}"),
            )
        })?,
    );
    Ok(resp)
}
