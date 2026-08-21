//! 出站 `chat/completions`：按厂商目录改写 `image_url` 内容块。
//!
//! 会话仍保存 **`/uploads/<文件名>`**。真正 HTTP 前：文本网关压成纯文本；视觉网关读盘打成 **`data:`** URL。

use std::path::{Path, PathBuf};

use crate::cm_llm::vendor_catalog::resolved_vendor_caps;
use crate::cm_types::{Message, MessageContent};

const MAX_INLINE_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const FLATTEN_PLACEHOLDER: &str = "（用户发送了图片，但当前模型不支持视觉输入。）";

/// 按 **`image_url_content_parts`** 改写 `messages`（就地）。应在请求 JSON 序列化之前、日志预览之后调用，避免把 base64 打进日志。
pub fn rewrite_messages_for_vendor(
    messages: &mut [Message],
    model: &str,
    api_base: &str,
    uploads_dir: Option<&Path>,
) {
    let allow = resolved_vendor_caps(model, api_base).image_url_content_parts;
    let mut budget = MAX_INLINE_IMAGE_BYTES;
    for msg in messages {
        rewrite_one_message(msg, allow, uploads_dir, &mut budget);
    }
}

fn rewrite_one_message(
    msg: &mut Message,
    allow: bool,
    uploads_dir: Option<&Path>,
    budget: &mut u64,
) {
    let Some(MessageContent::Parts(parts)) = msg.content.as_mut() else {
        return;
    };
    if allow {
        *parts = inline_image_parts(std::mem::take(parts), uploads_dir, budget);
        collapse_single_text_part(msg);
    } else {
        flatten_image_parts(msg);
    }
}

fn collapse_single_text_part(msg: &mut Message) {
    let Some(MessageContent::Parts(parts)) = &msg.content else {
        return;
    };
    if parts.len() != 1 {
        return;
    }
    let Some(obj) = parts[0].as_object() else {
        return;
    };
    let is_text = obj.get("type").and_then(|v| v.as_str()) == Some("text");
    if !is_text {
        return;
    }
    let Some(text) = obj.get("text").and_then(|v| v.as_str()) else {
        return;
    };
    msg.content = Some(MessageContent::Text(text.to_string()));
}

fn flatten_image_parts(msg: &mut Message) {
    let Some(MessageContent::Parts(parts)) = &msg.content else {
        return;
    };
    let mut texts = Vec::new();
    let mut dropped_image = false;
    for part in parts {
        let Some(obj) = part.as_object() else {
            continue;
        };
        let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if typ == "text" {
            if let Some(t) = obj.get("text").and_then(|v| v.as_str())
                && !t.trim().is_empty()
            {
                texts.push(t.to_string());
            }
        } else if typ == "image_url" {
            dropped_image = true;
        }
    }
    let mut body = texts.join("\n");
    if dropped_image && body.trim().is_empty() {
        body = FLATTEN_PLACEHOLDER.to_string();
    } else if dropped_image {
        body.push('\n');
        body.push_str(FLATTEN_PLACEHOLDER);
    }
    msg.content = Some(MessageContent::Text(body));
}

fn inline_image_parts(
    parts: Vec<serde_json::Value>,
    uploads_dir: Option<&Path>,
    budget: &mut u64,
) -> Vec<serde_json::Value> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        match rewrite_image_url_part(&part, uploads_dir, budget) {
            ImagePartOutcome::Keep(v) => out.push(v),
            ImagePartOutcome::SkipNote(note) => {
                out.push(serde_json::json!({"type": "text", "text": note}));
            }
        }
    }
    out
}

enum ImagePartOutcome {
    Keep(serde_json::Value),
    SkipNote(String),
}

fn rewrite_image_url_part(
    part: &serde_json::Value,
    uploads_dir: Option<&Path>,
    budget: &mut u64,
) -> ImagePartOutcome {
    let Some(obj) = part.as_object() else {
        return ImagePartOutcome::Keep(part.clone());
    };
    if obj.get("type").and_then(|v| v.as_str()) != Some("image_url") {
        return ImagePartOutcome::Keep(part.clone());
    }
    let Some(url) = obj
        .get("image_url")
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
    else {
        return ImagePartOutcome::Keep(part.clone());
    };
    let url = url.trim();
    if url.starts_with("data:") || looks_like_http_url(url) {
        return ImagePartOutcome::Keep(part.clone());
    }
    let Some(name) = uploads_file_name(url) else {
        return ImagePartOutcome::SkipNote(omit_note(InlineFail::BadPath, url));
    };
    let shown = format!("/uploads/{name}");
    let Some(dir) = uploads_dir else {
        return ImagePartOutcome::SkipNote(omit_note(InlineFail::NoDir, &shown));
    };
    match read_upload_as_data_url(dir, &name, budget) {
        Ok(data_url) => {
            let mut cloned = part.clone();
            if let Some(img) = cloned.get_mut("image_url").and_then(|v| v.as_object_mut()) {
                img.insert("url".into(), serde_json::Value::String(data_url));
            }
            ImagePartOutcome::Keep(cloned)
        }
        Err(fail) => ImagePartOutcome::SkipNote(omit_note(fail, &shown)),
    }
}

#[derive(Clone, Copy)]
enum InlineFail {
    BadPath,
    NoDir,
    Read,
    Empty,
    TooLarge,
    NotImage,
}

fn omit_note(fail: InlineFail, shown: &str) -> String {
    let why = match fail {
        InlineFail::TooLarge => "附图超过出站大小上限",
        InlineFail::NotImage => "附图不是 JPEG/PNG/GIF/WebP",
        InlineFail::Empty => "附图为空",
        InlineFail::NoDir | InlineFail::Read | InlineFail::BadPath => "附图无法读取",
    };
    format!("（{why}，已省略：{shown}）")
}

fn looks_like_http_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("https://") || u.starts_with("http://")
}

fn uploads_file_name(url: &str) -> Option<String> {
    let t = url.trim();
    if t.contains("..") || t.contains('\\') || t.contains("//") {
        return None;
    }
    let name = t.strip_prefix("/uploads/")?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some(name.to_string())
}

fn read_upload_as_data_url(
    dir: &Path,
    name: &str,
    budget: &mut u64,
) -> Result<String, InlineFail> {
    let path = safe_upload_path(dir, name).ok_or(InlineFail::BadPath)?;
    let bytes = std::fs::read(&path).map_err(|_| InlineFail::Read)?;
    let len = bytes.len() as u64;
    if len == 0 {
        return Err(InlineFail::Empty);
    }
    if len > *budget {
        return Err(InlineFail::TooLarge);
    }
    let mime = sniff_image_mime(&bytes).ok_or(InlineFail::NotImage)?;
    *budget -= len;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn safe_upload_path(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(dir.join(name))
}

fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if is_jpeg_magic(bytes) {
        return Some("image/jpeg");
    }
    if is_png_magic(bytes) {
        return Some("image/png");
    }
    if is_gif_magic(bytes) {
        return Some("image/gif");
    }
    if is_webp_magic(bytes) {
        return Some("image/webp");
    }
    None
}

fn is_jpeg_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

fn is_png_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn is_gif_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
}

fn is_webp_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::message_user_with_images;

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn flatten_drops_image_url_for_text_deepseek() {
        let mut msgs = vec![message_user_with_images("看图", &["/uploads/a.png".into()])];
        rewrite_messages_for_vendor(
            &mut msgs,
            "deepseek-v4-flash",
            "https://api.deepseek.com/v1",
            None,
        );
        let MessageContent::Text(t) = msgs[0].content.as_ref().expect("text") else {
            panic!("expected flattened text");
        };
        assert!(t.contains("看图"));
        assert!(t.contains("不支持视觉"));
        assert!(!t.contains("image_url"));
    }

    #[test]
    fn flatten_text_deepseek_on_proxy_host() {
        let mut msgs = vec![message_user_with_images("看图", &["/uploads/a.png".into()])];
        rewrite_messages_for_vendor(
            &mut msgs,
            "deepseek-v4-flash",
            "https://llm.example.com/v1",
            None,
        );
        let MessageContent::Text(t) = msgs[0].content.as_ref().expect("text") else {
            panic!("expected flattened text");
        };
        assert!(t.contains("不支持视觉"));
    }

    #[test]
    fn vision_inlines_png_as_data_url() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.png"), PNG_1X1).expect("write");
        let mut msgs = vec![message_user_with_images("描述", &["/uploads/a.png".into()])];
        rewrite_messages_for_vendor(
            &mut msgs,
            "deepseek-v4-flash-vision-exp",
            "https://api.deepseek.com/v1",
            Some(dir.path()),
        );
        let MessageContent::Parts(parts) = msgs[0].content.as_ref().expect("parts") else {
            panic!("expected parts");
        };
        let url = parts[1]["image_url"]["url"].as_str().expect("url");
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(parts[0]["text"].as_str() == Some("描述"));
    }

    #[test]
    fn vision_missing_file_becomes_note() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut msgs = vec![message_user_with_images("", &["/uploads/gone.png".into()])];
        rewrite_messages_for_vendor(
            &mut msgs,
            "deepseek-v4-flash-vision-exp",
            "https://api.deepseek.com/v1",
            Some(dir.path()),
        );
        match msgs[0].content.as_ref() {
            Some(MessageContent::Text(t)) => assert!(t.contains("无法读取")),
            Some(MessageContent::Parts(p)) => {
                let joined: String = p
                    .iter()
                    .filter_map(|v| v.get("text").and_then(|x| x.as_str()))
                    .collect();
                assert!(joined.contains("无法读取"));
            }
            _ => panic!("expected note"),
        }
    }

    #[test]
    fn vision_non_image_file_explains_type() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.png"), b"not-an-image").expect("write");
        let mut msgs = vec![message_user_with_images("", &["/uploads/a.png".into()])];
        rewrite_messages_for_vendor(
            &mut msgs,
            "deepseek-v4-flash-vision-exp",
            "https://api.deepseek.com/v1",
            Some(dir.path()),
        );
        let joined = match msgs[0].content.as_ref() {
            Some(MessageContent::Text(t)) => t.clone(),
            Some(MessageContent::Parts(p)) => p
                .iter()
                .filter_map(|v| v.get("text").and_then(|x| x.as_str()))
                .collect(),
            _ => panic!("expected note"),
        };
        assert!(joined.contains("不是 JPEG/PNG/GIF/WebP"));
    }

    #[test]
    fn over_budget_jpeg_is_too_large() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.jpg"), [0xFF, 0xD8, 0xFF, 0x01]).expect("write");
        let mut budget = 3;
        assert!(matches!(
            read_upload_as_data_url(dir.path(), "a.jpg", &mut budget),
            Err(InlineFail::TooLarge)
        ));
    }

    #[test]
    fn rejects_path_escape_in_uploads_name() {
        assert!(uploads_file_name("/uploads/../etc/passwd").is_none());
        assert!(uploads_file_name("/uploads/a/b.png").is_none());
        assert_eq!(
            uploads_file_name("/uploads/ok.png").as_deref(),
            Some("ok.png")
        );
    }
}
