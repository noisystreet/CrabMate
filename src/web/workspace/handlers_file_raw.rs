//! `GET /workspace/file/raw`：工作区内常见图片字节（供聊天气泡 `<img>` 经 Client 鉴权拉取）。

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::Json;

use super::handlers::effective_workspace_base_canonical;
#[cfg(unix)]
use super::handlers_sync::workspace_read_file_bytes_sync_unix;
use crate::web::app_state::AppStateHttpCore;
use crate::web::http_types::chat::ApiError;
use crate::web::http_types::workspace::WorkspaceFileQuery;
use crate::workspace::path::resolve_web_workspace_read_path;

/// 与 Web `POST /upload` 单张图片上限一致。
const WORKSPACE_IMAGE_RAW_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) type RawErr = (StatusCode, Json<ApiError>);

pub(crate) fn raw_err(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> RawErr {
    (status, Json(ApiError::new(code, message)))
}

/// 工作区相对路径中允许作为聊天内嵌图的扩展名（不含 svg，避免内联脚本）。
pub(crate) fn workspace_chat_image_content_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

pub(crate) fn reject_unsafe_rel_path(path: &str) -> Result<(), RawErr> {
    let t = path.trim();
    if t.is_empty() {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_PATH_EMPTY",
            "path 不能为空",
        ));
    }
    if t.contains('\\') {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            "WORKSPACE_PATH_INVALID",
            "path 非法",
        ));
    }
    for part in t.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_PATH_INVALID",
                "path 非法",
            ));
        }
    }
    Ok(())
}

/// 读取工作区内图片原始字节；失败为 JSON `ApiError`（与 upload 同类）。
pub async fn workspace_file_raw_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Result<Response, RawErr> {
    let path = query.path.trim();
    reject_unsafe_rel_path(path)?;
    let Some(ctype) = workspace_chat_image_content_type(path) else {
        return Err(raw_err(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "WORKSPACE_IMAGE_UNSUPPORTED",
            "仅支持 png/jpg/jpeg/webp/gif",
        ));
    };
    let bytes = load_workspace_rel_file_bytes(
        &http,
        path,
        WORKSPACE_IMAGE_RAW_MAX_BYTES,
        "WORKSPACE_IMAGE_READ",
    )
    .await?;
    bytes_ok_response(
        bytes,
        ctype,
        "private, max-age=60",
        "WORKSPACE_IMAGE_RESPONSE",
    )
}

pub(crate) fn bytes_ok_response(
    bytes: Vec<u8>,
    ctype: &'static str,
    cache: &'static str,
    err_code: &'static str,
) -> Result<Response, RawErr> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::CACHE_CONTROL, cache)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(bytes))
        .map_err(|e| {
            raw_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                err_code,
                format!("构造响应失败: {e}"),
            )
        })
}

/// 已通过 [`reject_unsafe_rel_path`] 的相对路径：解析并读取字节。
pub(crate) async fn load_workspace_rel_file_bytes(
    http: &AppStateHttpCore,
    path: &str,
    max_b: u64,
    read_code: &'static str,
) -> Result<Vec<u8>, RawErr> {
    let base = effective_workspace_base_canonical(http)
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::BAD_REQUEST,
                "WORKSPACE_UNAVAILABLE",
                e.user_message(),
            )
        })?;
    let canonical = resolve_web_workspace_read_path(&base, Some(path)).map_err(|e| {
        let status = if e.is_policy_denied() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        raw_err(status, "WORKSPACE_PATH_DENIED", e.user_message())
    })?;
    read_workspace_bytes(base, canonical, max_b, read_code).await
}

async fn read_workspace_bytes(
    base: std::path::PathBuf,
    canonical: std::path::PathBuf,
    max_b: u64,
    read_code: &'static str,
) -> Result<Vec<u8>, RawErr> {
    #[cfg(unix)]
    {
        tokio::task::spawn_blocking(move || {
            workspace_read_file_bytes_sync_unix(base, canonical, max_b)
        })
        .await
        .map_err(|e| {
            raw_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                read_code,
                format!("读取文件任务失败: {e}"),
            )
        })?
        .map_err(|msg| map_read_msg_to_status(msg, read_code))
    }
    #[cfg(not(unix))]
    {
        let _ = base;
        read_workspace_bytes_non_unix(canonical, max_b, read_code).await
    }
}

fn map_read_msg_to_status(msg: String, code: &'static str) -> RawErr {
    let status = if msg.contains("过大") {
        StatusCode::PAYLOAD_TOO_LARGE
    } else if msg.contains("目录") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::NOT_FOUND
    };
    raw_err(status, code, msg)
}

#[cfg(not(unix))]
async fn read_workspace_bytes_non_unix(
    canonical: std::path::PathBuf,
    max_b: u64,
    read_code: &'static str,
) -> Result<Vec<u8>, RawErr> {
    let meta = tokio::fs::metadata(&canonical).await.map_err(|e| {
        raw_err(
            StatusCode::NOT_FOUND,
            read_code,
            format!("无法读取文件信息: {e}"),
        )
    })?;
    if meta.is_dir() {
        return Err(raw_err(
            StatusCode::BAD_REQUEST,
            read_code,
            "路径是目录，无法读取为文件",
        ));
    }
    if meta.len() > max_b {
        return Err(raw_err(
            StatusCode::PAYLOAD_TOO_LARGE,
            read_code,
            format!("文件过大（{} 字节），当前最多读取 {} 字节", meta.len(), max_b),
        ));
    }
    tokio::fs::read(&canonical).await.map_err(|e| {
        raw_err(
            StatusCode::NOT_FOUND,
            read_code,
            format!("读取文件失败: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{reject_unsafe_rel_path, workspace_chat_image_content_type};

    #[test]
    fn image_ext_allowlist() {
        assert_eq!(
            workspace_chat_image_content_type("plots/a.PNG"),
            Some("image/png")
        );
        assert_eq!(
            workspace_chat_image_content_type("x.jpeg"),
            Some("image/jpeg")
        );
        assert_eq!(workspace_chat_image_content_type("x.svg"), None);
        assert_eq!(workspace_chat_image_content_type("x.rs"), None);
    }

    #[test]
    fn rejects_dotdot() {
        assert!(reject_unsafe_rel_path("../x.png").is_err());
        assert!(reject_unsafe_rel_path("a/../b.png").is_err());
        assert!(reject_unsafe_rel_path("a/./b.png").is_err());
        assert!(reject_unsafe_rel_path("ok/a.png").is_ok());
        assert!(reject_unsafe_rel_path("foo..bar.png").is_ok());
    }
}
