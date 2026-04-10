//! 将**上游 HTTP 响应体**等长文本截断为适合 **`log` 输出**的预览，避免把全文写入日志。
//!
//! 与仓库「密钥与日志脱敏」规则配合：**不得**在 `error!` / 返回给前端的 `Err` 中附带完整供应商响应体；
//! 排障时使用 `body_preview` + `body_len` 即可。
//!
//! 发往模型的 **`ChatRequest` JSON** 预览：由 `llm::api::stream_chat` 输出（长度上限见 [`CHAT_REQUEST_JSON_LOG_MAX_CHARS`]）。
//! - 设置 **`RUST_LOG=crabmate=debug`**（或更宽 `debug`）时走 **`debug!`**；
//! - 仅 **`--log` 文件**且默认 **info** 时：设环境变量 **`AGENT_LOG_CHAT_REQUEST_JSON=1`** 则走 **`info!`**，否则不打印。
//!
//! 对话/助手消息预览用于 `log::debug!`：默认仅开启 `RUST_LOG=debug` 时输出，且始终截断。

use crate::types::{Message, message_content_as_str};

/// 日志里展示的响应体预览最大字符数（Unicode 标量）。
pub const HTTP_BODY_PREVIEW_LOG_CHARS: usize = 256;

/// `stream_chat` 发往供应商前，DEBUG 日志中 **`ChatRequest` JSON** 的最大字符数（Unicode 标量）。
/// 仅用于排障；完整 tools 定义可能很长，超出部分见 `…(truncated)`。
pub const CHAT_REQUEST_JSON_LOG_MAX_CHARS: usize = 12_288;

/// 从供应商 JSON 里取出 `error.message` 后，写入**用户可见** `Err` 的最大长度（不含 HTTP 状态前缀）。
/// 仅拼接解析出的文案，不附带整段 body（见模块顶部说明）。
pub const CHAT_API_USER_ERROR_MSG_CHARS: usize = 180;

/// 对话消息写入日志时的正文预览长度（HTTP/CLI 等仍截断处使用）。
/// 部分高噪声调试目标（如终端 UI 专用 log target）的会话输出全文，不使用本长度。
pub const MESSAGE_LOG_PREVIEW_CHARS: usize = 320;

/// 按 Unicode 标量截断；超出则后缀 `…(truncated)`。
pub fn preview_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut iter = s.chars();
    let prefix: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{prefix}…(truncated)")
    } else {
        prefix
    }
}

/// 将空白（含换行、制表）规范为**单空格**后截断，便于结构化日志单行输出。
pub fn single_line_preview(s: &str, max_chars: usize) -> String {
    let folded = s.split_whitespace().collect::<Vec<_>>().join(" ");
    preview_chars(&folded, max_chars)
}

/// 从 OpenAI 兼容的 chat 错误 JSON 中取出简短 `message`，供 `stream_chat` 等返回给 TUI/前端。
/// 解析失败或非字符串字段时返回 `None`（调用方保留泛化提示语）。
pub fn chat_api_error_message_for_user(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let msg = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .or_else(|| v.get("message").and_then(|m| m.as_str()))?;
    let msg = msg.trim();
    if msg.is_empty() {
        return None;
    }
    Some(single_line_preview(msg, CHAT_API_USER_ERROR_MSG_CHARS))
}

/// 从消息列表中取**最后一条** `user` 角色正文的截断预览，供调试日志使用。
pub fn last_user_message_preview_for_log(messages: &[Message]) -> String {
    for m in messages.iter().rev() {
        if m.role == "user" {
            return match message_content_as_str(&m.content).map(str::trim) {
                None | Some("") => "<empty>".to_string(),
                Some(s) => preview_chars(s, MESSAGE_LOG_PREVIEW_CHARS),
            };
        }
    }
    "<no user>".to_string()
}

/// 单条助手（或其它角色）消息摘要：正文截断 + 若有 `tool_calls` 则附工具名（参数不全文记录）。
pub fn assistant_message_preview_for_log(msg: &Message) -> String {
    let content_p = match message_content_as_str(&msg.content).map(str::trim) {
        None | Some("") => None,
        Some(s) => Some(preview_chars(s, MESSAGE_LOG_PREVIEW_CHARS)),
    };
    let tool_names = msg.tool_calls.as_ref().map(|tcs| {
        tcs.iter()
            .map(|tc| tc.function.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    });
    let tools_nonempty = tool_names.as_deref().filter(|t| !t.is_empty());
    match (&content_p, tools_nonempty) {
        (None, None) => "<empty>".to_string(),
        (Some(c), None) => c.clone(),
        (None, Some(t)) => format!("(no text) tools=[{t}]"),
        (Some(c), Some(t)) => format!("{c} | tools=[{t}]"),
    }
}

/// 工具调用 `arguments` JSON 字符串的日志预览（防过长、防误打满屏）。
pub fn tool_arguments_preview_for_log(args: &str) -> String {
    preview_chars(args, 240)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_with_marker() {
        let s = "a".repeat(10);
        assert_eq!(preview_chars(&s, 5), "aaaaa…(truncated)");
        assert_eq!(preview_chars("hi", 10), "hi");
    }

    #[test]
    fn single_line_collapses_newlines() {
        assert_eq!(single_line_preview("a\nb\r\nc", 20), "a b c");
        assert_eq!(single_line_preview("  x  \t y  ", 20), "x y");
    }

    #[test]
    fn chat_api_error_message_parses_openai_shape() {
        let body = r#"{"error":{"message":"Invalid model","type":"invalid_request_error"}}"#;
        assert_eq!(
            chat_api_error_message_for_user(body).as_deref(),
            Some("Invalid model")
        );
    }

    #[test]
    fn chat_api_error_message_missing_returns_none() {
        assert_eq!(chat_api_error_message_for_user("not json"), None);
        assert_eq!(chat_api_error_message_for_user("{}"), None);
    }

    #[test]
    fn last_user_preview_finds_last_user() {
        use crate::types::Message;
        let msgs = vec![
            Message::system_only("s"),
            Message::user_only("first"),
            Message::user_only("second"),
        ];
        assert!(last_user_message_preview_for_log(&msgs).contains("second"));
    }
}
