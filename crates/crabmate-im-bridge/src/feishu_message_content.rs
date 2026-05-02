//! 将飞书 **`im.message.receive_v1`** 的 **`message_type` + `content`（JSON 字符串）** 转为送入 CrabMate 的**用户侧纯文本**。
//!
//! 权威字段说明见飞书文档：[接收消息内容](https://open.feishu.cn/document/server-docs/im-v1/message/events/message_content)。

use serde_json::Value;

/// 把 `content` JSON 解析为单行或多行 UTF-8 文本；无法识别或为空时返回 **`None`**。
pub fn incoming_content_as_user_text(
    message_type: &str,
    content_json: &str,
    max_json_chars: usize,
) -> Option<String> {
    let v: Value = serde_json::from_str(content_json).ok()?;
    let max_json = max_json_chars.max(256);

    let raw = match message_type {
        "text" => v.get("text").and_then(|t| t.as_str()).map(str::to_string)?,
        "post" => {
            let mut buf = String::new();
            collect_post_rich_text(&v, &mut buf);
            if buf.trim().is_empty() {
                return None;
            }
            buf
        }
        "image" | "sticker" => {
            let key = v.get("image_key").and_then(|x| x.as_str()).unwrap_or("");
            format!("[飞书{message_type}消息，无法在此直接展示图片；image_key={key}]")
        }
        "file" => {
            let name = v.get("file_name").and_then(|x| x.as_str()).unwrap_or("");
            let key = v.get("file_key").and_then(|x| x.as_str()).unwrap_or("");
            format!("[飞书文件消息 file_name={name} file_key={key}]")
        }
        "audio" => {
            let key = v.get("file_key").and_then(|x| x.as_str()).unwrap_or("");
            format!("[飞书语音消息 file_key={key}]")
        }
        "media" => {
            let key = v.get("file_key").and_then(|x| x.as_str()).unwrap_or("");
            let name = v.get("file_name").and_then(|x| x.as_str()).unwrap_or("");
            format!("[飞书视频消息 file_name={name} file_key={key}]")
        }
        "interactive" => {
            let dump = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
            format!(
                "[飞书消息卡片 interactive，JSON 摘要]\n{}",
                clip_chars(&dump, max_json)
            )
        }
        "share_chat" | "share_user" => {
            let dump = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
            format!(
                "[飞书{message_type} 名片/会话分享]\n{}",
                clip_chars(&dump, max_json)
            )
        }
        other => {
            let dump = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
            format!(
                "[飞书消息类型 {other}，原始 content 摘要]\n{}",
                clip_chars(&dump, max_json)
            )
        }
    };

    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn clip_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let take = max.saturating_sub(20);
    s.chars().take(take).collect::<String>() + "\n…（JSON 已截断）"
}

/// 递归收集富文本 **`{"tag":"text","text":"…"}`** 与常见 **`title`** 字段（`post` 多语言结构）。
fn collect_post_rich_text(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            if let Some("text") = map.get("tag").and_then(|t| t.as_str())
                && let Some(t) = map.get("text").and_then(|x| x.as_str())
                && !t.is_empty()
            {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            }
            if let Some(title) = map.get("title").and_then(|x| x.as_str())
                && !title.is_empty()
            {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(title);
            }
            for (_k, val) in map {
                collect_post_rich_text(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_post_rich_text(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_message() {
        let s = incoming_content_as_user_text("text", r#"{"text":"  hi  "}"#, 1000).unwrap();
        assert_eq!(s, "hi");
    }

    #[test]
    fn post_zh_cn_text_tags() {
        let json = r#"{
            "zh_cn": {
                "title": "标题",
                "content": [
                    [{"tag": "text", "text": "第一行"}],
                    [{"tag": "text", "text": "第二行"}]
                ]
            }
        }"#;
        let s = incoming_content_as_user_text("post", json, 1000).unwrap();
        assert!(s.contains("标题"));
        assert!(s.contains("第一行"));
        assert!(s.contains("第二行"));
    }

    #[test]
    fn image_placeholder() {
        let s =
            incoming_content_as_user_text("image", r#"{"image_key":"img_v1_abc"}"#, 1000).unwrap();
        assert!(s.contains("img_v1_abc"));
    }
}
