//! `POST /upload`、`GET /uploads/{filename}`、`POST /upload/delete` 与 uploads 目录清理。

use std::collections::HashSet;

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use log::error;
use tokio::io::AsyncWriteExt;

use crate::web::app_state_facets::UploadsFacet;
use crate::web::http_types::chat::{
    ApiError, DeleteUploadsBody, DeleteUploadsResponseBody, UploadResponseBody, UploadedFileInfo,
};

type UploadErr = (StatusCode, Json<ApiError>);

fn upload_api_error(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> UploadErr {
    (status, Json(ApiError::new(code, message)))
}

fn upload_max_single_bytes(file_name: &str, mime: &str) -> Result<u64, UploadErr> {
    let ext = ext_lower(file_name).unwrap_or_default();
    let is_image = mime.starts_with("image/")
        && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif");
    let is_audio = mime.starts_with("audio/")
        && matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "aac" | "ogg" | "webm");
    let is_video =
        mime.starts_with("video/") && matches!(ext.as_str(), "mp4" | "webm" | "mov" | "mkv");
    if !(is_image || is_audio || is_video) {
        return Err(upload_api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UPLOAD_UNSUPPORTED_TYPE",
            "不支持的文件类型（仅支持常见图片/音频/视频）",
        ));
    }
    Ok(if is_image {
        8 * 1024 * 1024
    } else if is_audio {
        25 * 1024 * 1024
    } else {
        80 * 1024 * 1024
    })
}

pub(crate) async fn delete_uploads_handler(
    State(facet): State<UploadsFacet>,
    Json(body): Json<DeleteUploadsBody>,
) -> Result<Json<DeleteUploadsResponseBody>, (StatusCode, Json<ApiError>)> {
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    for u in body.urls {
        // 只接受 /uploads/<filename> 形式，避免目录穿越
        if !u.starts_with("/uploads/") || u.contains("..") || u.contains('\\') {
            skipped.push(u);
            continue;
        }
        let name = u.trim_start_matches("/uploads/");
        if name.is_empty() || name.contains('/') {
            skipped.push(u);
            continue;
        }
        let path = facet.uploads_dir().await.join(name);
        // 不暴露更多信息：不存在也当作 skipped
        match tokio::fs::remove_file(&path).await {
            Ok(()) => deleted.push(format!("/uploads/{}", name)),
            Err(_) => skipped.push(format!("/uploads/{}", name)),
        }
    }
    Ok(Json(DeleteUploadsResponseBody { deleted, skipped }))
}

type UploadEntry = (std::path::PathBuf, std::time::SystemTime, u64);

async fn collect_upload_entries(dir: &std::path::Path) -> Option<(Vec<UploadEntry>, u64)> {
    let now = std::time::SystemTime::now();
    let mut entries: Vec<UploadEntry> = Vec::new();
    let mut total: u64 = 0;
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(r) => r,
        Err(e) => {
            error!(
                target: "crabmate",
                "uploads 清理：无法读取目录 dir={} error={}",
                dir.display(),
                e
            );
            return None;
        }
    };
    while let Ok(Some(ent)) = rd.next_entry().await {
        let path = ent.path();
        let meta = match ent.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let mtime = meta.modified().unwrap_or(now);
        total = total.saturating_add(size);
        entries.push((path, mtime, size));
    }
    Some((entries, total))
}

async fn purge_uploads_by_age(
    entries: Vec<UploadEntry>,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
    total: &mut u64,
    referenced: &HashSet<String>,
) -> Vec<UploadEntry> {
    let mut kept = Vec::new();
    for (p, mt, sz) in entries {
        if upload_file_name(&p)
            .map(|n| referenced.contains(&n))
            .unwrap_or(false)
        {
            kept.push((p, mt, sz));
            continue;
        }
        let too_old = now
            .duration_since(mt)
            .ok()
            .map(|d| d > max_age)
            .unwrap_or(false);
        if too_old {
            if tokio::fs::remove_file(&p).await.is_ok() {
                *total = total.saturating_sub(sz);
            }
        } else {
            kept.push((p, mt, sz));
        }
    }
    kept
}

async fn purge_uploads_by_bytes(
    kept: Vec<UploadEntry>,
    max_bytes: u64,
    total: &mut u64,
    referenced: &HashSet<String>,
) {
    if *total <= max_bytes {
        return;
    }
    let mut kept = kept;
    kept.sort_by_key(|x| x.1);
    for (p, _mt, sz) in kept {
        if *total <= max_bytes {
            break;
        }
        if upload_file_name(&p)
            .map(|n| referenced.contains(&n))
            .unwrap_or(false)
        {
            continue;
        }
        if tokio::fs::remove_file(&p).await.is_ok() {
            *total = total.saturating_sub(sz);
        }
    }
}

fn upload_file_name(p: &std::path::Path) -> Option<String> {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
}

pub(crate) async fn cleanup_uploads_dir(
    dir: std::path::PathBuf,
    max_age: std::time::Duration,
    max_bytes: u64,
    referenced: &HashSet<String>,
) {
    let now = std::time::SystemTime::now();
    let Some((entries, mut total)) = collect_upload_entries(&dir).await else {
        return;
    };
    let kept = purge_uploads_by_age(entries, now, max_age, &mut total, referenced).await;
    purge_uploads_by_bytes(kept, max_bytes, &mut total, referenced).await;
}

fn sanitize_display_filename(input: &str) -> String {
    // 仅用于“展示给前端”，不参与落盘路径（落盘用服务端生成的 safe_name）
    let base = std::path::Path::new(input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin");
    let mut out = String::with_capacity(base.len().min(80));
    for ch in base.chars() {
        let ok = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '-' | '_' | ' ' | '(' | ')' | '[' | ']');
        out.push(if ok { ch } else { '_' });
        if out.len() >= 80 {
            break;
        }
    }
    if out.trim().is_empty() {
        "upload.bin".to_string()
    } else {
        out
    }
}

fn ext_lower(file_name: &str) -> Option<String> {
    std::path::Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
}

async fn write_upload_field_chunks(
    field: &mut axum::extract::multipart::Field<'_>,
    path: &std::path::Path,
    f: &mut tokio::fs::File,
    max_single: u64,
    total: &mut u64,
    max_total: u64,
) -> Result<u64, UploadErr> {
    let mut size: u64 = 0;
    loop {
        let next = match field.chunk().await {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_file(path).await;
                return Err(upload_api_error(
                    StatusCode::BAD_REQUEST,
                    "UPLOAD_READ_ERROR",
                    format!("读取上传内容失败：{}", e),
                ));
            }
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk_len = chunk.len() as u64;
        size += chunk_len;
        *total += chunk_len;
        if size > max_single {
            let _ = tokio::fs::remove_file(path).await;
            return Err(upload_api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "UPLOAD_FILE_TOO_LARGE",
                "单个文件过大",
            ));
        }
        if *total > max_total {
            let _ = tokio::fs::remove_file(path).await;
            return Err(upload_api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "UPLOAD_TOO_LARGE",
                "上传内容过大",
            ));
        }
        f.write_all(&chunk).await.map_err(|e| {
            upload_api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_WRITE_ERROR",
                format!("写入上传内容失败：{}", e),
            )
        })?;
    }
    Ok(size)
}

fn upload_safe_disk_name(file_name: &str) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let ext = ext_lower(file_name).unwrap_or_default();
    let ext_with_dot = if ext.is_empty() {
        "".to_string()
    } else {
        format!(".{}", ext)
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("u{}_{}_{}{}", std::process::id(), ts, n, ext_with_dot)
}

async fn store_one_upload_field(
    facet: &UploadsFacet,
    field: axum::extract::multipart::Field<'_>,
    total: &mut u64,
    max_total: u64,
) -> Result<UploadedFileInfo, UploadErr> {
    let raw_name = field.file_name().unwrap_or("upload.bin");
    let file_name = sanitize_display_filename(raw_name);
    let mime = field
        .content_type()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let max_single = upload_max_single_bytes(&file_name, &mime)?;
    let safe_name = upload_safe_disk_name(&file_name);
    let path = facet.uploads_dir().await.join(&safe_name);
    let mut f = tokio::fs::File::create(&path).await.map_err(|e| {
        upload_api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UPLOAD_WRITE_ERROR",
            format!("无法写入上传文件：{}", e),
        )
    })?;
    let mut field = field;
    let size =
        write_upload_field_chunks(&mut field, &path, &mut f, max_single, total, max_total).await?;
    Ok(UploadedFileInfo {
        url: format!("/uploads/{}", safe_name),
        filename: file_name,
        mime,
        size,
    })
}

pub(crate) async fn upload_handler(
    State(facet): State<UploadsFacet>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponseBody>, UploadErr> {
    let mut out: Vec<UploadedFileInfo> = Vec::new();
    let max_total: u64 = 200 * 1024 * 1024; // 200MB total
    let max_files: usize = 20;
    let mut total: u64 = 0;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        upload_api_error(
            StatusCode::BAD_REQUEST,
            "MULTIPART_ERROR",
            format!("上传解析失败：{}", e),
        )
    })? {
        if out.len() >= max_files {
            return Err(upload_api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "UPLOAD_TOO_MANY_FILES",
                "上传文件数量过多",
            ));
        }
        out.push(store_one_upload_field(&facet, field, &mut total, max_total).await?);
    }

    Ok(Json(UploadResponseBody { files: out }))
}

const GET_UPLOAD_MAX_BYTES: u64 = 80 * 1024 * 1024;

fn upload_filename_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn upload_content_type(name: &str, bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return "image/jpeg";
    }
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return "image/gif";
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    match ext_lower(name).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a" | "aac") => "audio/mp4",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

/// `GET /uploads/{filename}`：与其它受保护 API 同鉴权（不再走匿名 ServeDir）。
pub(crate) async fn get_upload_file_handler(
    State(facet): State<UploadsFacet>,
    Path(filename): Path<String>,
) -> Result<Response, UploadErr> {
    if !upload_filename_ok(&filename) {
        return Err(upload_api_error(
            StatusCode::BAD_REQUEST,
            "UPLOAD_PATH_INVALID",
            "非法文件名",
        ));
    }
    let dir = facet.uploads_dir().await;
    let path = dir.join(&filename);
    let meta = tokio::fs::metadata(&path).await.map_err(|_| {
        upload_api_error(StatusCode::NOT_FOUND, "UPLOAD_NOT_FOUND", "附图不存在或已过期")
    })?;
    if !meta.is_file() {
        return Err(upload_api_error(
            StatusCode::NOT_FOUND,
            "UPLOAD_NOT_FOUND",
            "附图不存在或已过期",
        ));
    }
    if meta.len() > GET_UPLOAD_MAX_BYTES {
        return Err(upload_api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "UPLOAD_FILE_TOO_LARGE",
            "单个文件过大",
        ));
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| {
        upload_api_error(StatusCode::NOT_FOUND, "UPLOAD_NOT_FOUND", "附图不存在或已过期")
    })?;
    let ctype = upload_content_type(&filename, &bytes);
    let corp = {
        let g = facet.cfg.read().await;
        if g.web_api.web_cors_allowed_origins.is_empty() {
            "same-site"
        } else {
            "cross-origin"
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::CACHE_CONTROL, "private, max-age=60")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static(corp),
        )
        .body(Body::from(bytes))
        .map_err(|e| {
            upload_api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_READ_ERROR",
                format!("构造响应失败：{e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_get_filename() {
        assert!(!upload_filename_ok("../a.png"));
        assert!(!upload_filename_ok("a/b.png"));
        assert!(upload_filename_ok("u1_2_3.png"));
    }
}
