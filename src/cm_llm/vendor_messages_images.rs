//! 出站多模态 `content`：文本网关 flatten `image_url`；视觉网关把 `/uploads/` 读成 `data:` URL。
//!
//! 会话落盘仍用相对路径；本模块只改 **`ChatRequest.messages`**。

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use crate::cm_types::{Message, MessageContent};

/// 与 `serve` 默认上传目录一致（`std::env::temp_dir()` 下的 `crabmate_uploads`）。
#[must_use]
pub fn default_chat_uploads_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("crabmate_uploads")
}

const MAX_INLINE_UPLOAD_BYTES: u64 = 8 * 1024 * 1024;

fn content_part_image_url(part: &serde_json::Value) -> Option<&str> {
    let iu = part.get("image_url")?;
    if let Some(s) = iu.as_str() {
        return Some(s);
    }
    iu.get("url").and_then(|u| u.as_str())
}

fn image_url_label_for_placeholder(url: &str) -> &str {
    if url.is_empty() {
        "(未提供 URL)"
    } else if url.starts_with("data:") {
        "data URL"
    } else {
        url
    }
}

fn vision_unavailable_placeholder(url: &str) -> String {
    format!(
        "[用户附带图片 {}；当前模型无法查看像素，只能根据文字说明推断]",
        image_url_label_for_placeholder(url)
    )
}

fn upload_unread_placeholder(url: &str) -> String {
    format!(
        "[用户附带图片 {}；未能读入本地上传文件，当前请求未发送图像数据]",
        image_url_label_for_placeholder(url)
    )
}

fn flatten_parts_to_text(parts: &[serde_json::Value]) -> String {
    let mut chunks = Vec::new();
    for part in parts {
        match part.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "text" => {
                if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                    let t = t.trim();
                    if !t.is_empty() {
                        chunks.push(t.to_string());
                    }
                }
            }
            "image_url" => {
                let url = content_part_image_url(part).unwrap_or("");
                chunks.push(vision_unavailable_placeholder(url));
            }
            _ => {}
        }
    }
    chunks.join("\n\n")
}

fn flatten_image_url_parts_in_message(msg: &mut Message) {
    let Some(MessageContent::Parts(parts)) = &msg.content else {
        return;
    };
    let text = flatten_parts_to_text(parts);
    msg.content = Some(MessageContent::Text(text));
}

/// 将 `content` 数组压成字符串，去掉上游不认识的 **`image_url`** 变体。
pub fn flatten_image_url_parts_in_messages(messages: &mut [Message]) {
    for m in messages {
        flatten_image_url_parts_in_message(m);
    }
}

fn uploads_filename(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("/uploads/")?;
    if rest.is_empty() || rest.contains('/') || rest.contains("..") || rest.contains('\\') {
        return None;
    }
    Some(rest)
}

fn mime_for_upload_name(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn try_data_url_for_upload(uploads_dir: &Path, name: &str) -> Option<String> {
    let mime = mime_for_upload_name(name)?;
    let path = uploads_dir.join(name);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_INLINE_UPLOAD_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(format!("data:{mime};base64,{}", B64.encode(bytes)))
}

fn rewrite_image_url_part(part: &mut serde_json::Value, uploads_dir: &Path) {
    if part.get("type").and_then(|t| t.as_str()) != Some("image_url") {
        return;
    }
    let Some(url) = content_part_image_url(part).map(str::to_string) else {
        *part = serde_json::json!({"type": "text", "text": upload_unread_placeholder("")});
        return;
    };
    if !url.starts_with("/uploads/") {
        return;
    }
    let Some(name) = uploads_filename(&url) else {
        *part = serde_json::json!({
            "type": "text",
            "text": upload_unread_placeholder(&url)
        });
        return;
    };
    match try_data_url_for_upload(uploads_dir, name) {
        Some(data) => {
            if let Some(obj) = part.get_mut("image_url") {
                if obj.is_string() {
                    *obj = serde_json::Value::String(data);
                } else if let Some(map) = obj.as_object_mut() {
                    map.insert("url".to_string(), serde_json::Value::String(data));
                }
            }
        }
        None => {
            *part = serde_json::json!({
                "type": "text",
                "text": upload_unread_placeholder(&url)
            });
        }
    }
}

fn inline_one_message_uploads(msg: &mut Message, uploads_dir: &Path) {
    let Some(MessageContent::Parts(parts)) = &mut msg.content else {
        return;
    };
    for part in parts.iter_mut() {
        rewrite_image_url_part(part, uploads_dir);
    }
}

/// 把会话里的 **`/uploads/<文件名>`** 换成 `data:` URL，供远程视觉模型拉取像素。
///
/// 读失败或非图片扩展名时改为文字占位，避免把本机相对路径发给上游。
pub fn inline_local_chat_upload_images(messages: &mut [Message], uploads_dir: &Path) {
    for m in messages {
        inline_one_message_uploads(m, uploads_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::message_user_with_images;

    #[test]
    fn flatten_keeps_text_and_replaces_image_url() {
        let mut msgs = vec![message_user_with_images("看这张图", &["/uploads/a.png".into()])];
        flatten_image_url_parts_in_messages(&mut msgs);
        match &msgs[0].content {
            Some(MessageContent::Text(s)) => {
                assert!(s.contains("看这张图"));
                assert!(s.contains("/uploads/a.png"));
                assert!(s.contains("无法查看像素"));
                assert!(!s.contains("image_url"));
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn flatten_is_idempotent_on_plain_text() {
        let mut msgs = vec![Message::user_only("hello")];
        flatten_image_url_parts_in_messages(&mut msgs);
        assert_eq!(
            msgs[0].content,
            Some(MessageContent::Text("hello".to_string()))
        );
    }

    #[test]
    fn inline_rewrites_upload_path_to_data_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let png = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52,
        ];
        std::fs::write(dir.path().join("tiny.png"), png).expect("write");
        let mut msgs = vec![message_user_with_images("cap", &["/uploads/tiny.png".into()])];
        inline_local_chat_upload_images(&mut msgs, dir.path());
        let Some(MessageContent::Parts(parts)) = &msgs[0].content else {
            panic!("expected Parts");
        };
        let url = content_part_image_url(&parts[1]).expect("url");
        assert!(url.starts_with("data:image/png;base64,"), "url={url}");
        assert!(!url.contains("/uploads/"));
    }

    #[test]
    fn inline_missing_file_becomes_text_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut msgs = vec![message_user_with_images("", &["/uploads/missing.png".into()])];
        inline_local_chat_upload_images(&mut msgs, dir.path());
        let Some(MessageContent::Parts(parts)) = &msgs[0].content else {
            panic!("expected Parts");
        };
        assert_eq!(parts[0]["type"], "text");
        let t = parts[0]["text"].as_str().unwrap();
        assert!(t.contains("未能读入本地上传文件"));
        assert!(t.contains("/uploads/missing.png"));
    }
}
