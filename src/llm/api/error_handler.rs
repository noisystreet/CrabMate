//! `chat/completions` HTTP 非成功体、非流式 JSON 解析失败等：日志与用户可见错误（退避重试仍由 [`super::super::complete_chat_retrying`] 处理）。

use log::{debug, error, info};

use crate::redact::{
    self, CHAT_REQUEST_JSON_LOG_INFO_CHARS, CHAT_REQUEST_JSON_LOG_MAX_CHARS,
    HTTP_BODY_PREVIEW_LOG_CHARS,
};
use crate::types::ChatRequest;

use super::super::call_error::LlmCallError;

/// 在未开启 `RUST_LOG=…debug` 时，仍可用 **`AGENT_LOG_CHAT_REQUEST_JSON=1`** 在 **info** 级别打印请求体预览（与 `--log` 默认 `info` 配套）。
fn should_log_chat_request_json_preview() -> bool {
    log::log_enabled!(log::Level::Debug)
        || std::env::var_os("AGENT_LOG_CHAT_REQUEST_JSON").is_some_and(|v| {
            let s = v.to_string_lossy();
            let s = s.trim();
            !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
        })
}

pub(super) fn log_chat_request_json_preview_if_enabled(req: &ChatRequest) {
    if !should_log_chat_request_json_preview() {
        return;
    }
    let as_debug = log::log_enabled!(log::Level::Debug);
    match serde_json::to_string(req) {
        Ok(body) => {
            if as_debug {
                let preview = redact::preview_chars(&body, CHAT_REQUEST_JSON_LOG_MAX_CHARS);
                debug!(
                    target: "crabmate",
                    "chat 请求体 JSON len={} messages_count={} body_preview={}",
                    body.len(),
                    req.messages.len(),
                    preview
                );
            } else {
                let preview = redact::preview_chars(&body, CHAT_REQUEST_JSON_LOG_INFO_CHARS);
                info!(
                    target: "crabmate",
                    "chat 请求体 JSON len={} messages_count={} body_preview={}",
                    body.len(),
                    req.messages.len(),
                    preview
                );
            }
        }
        Err(e) => {
            if as_debug {
                debug!(
                    target: "crabmate",
                    "chat 请求体 JSON 序列化失败 err={}",
                    e
                );
            } else {
                info!(
                    target: "crabmate",
                    "chat 请求体 JSON 序列化失败 err={}",
                    e
                );
            }
        }
    }
}

pub(super) async fn ensure_chat_completions_success(
    res: reqwest::Response,
) -> Result<reqwest::Response, LlmCallError> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let preview = redact::single_line_preview(&body, HTTP_BODY_PREVIEW_LOG_CHARS);
    error!(
        target: "crabmate",
        "chat completions API 返回非成功状态 status={} body_len={} body_preview={}",
        status,
        body.len(),
        preview
    );
    let code = status.as_u16();
    let err_text = match redact::chat_api_error_message_for_user(&body) {
        Some(m) => format!("模型接口返回错误（HTTP {code}）：{m}"),
        None => format!("模型接口返回错误（HTTP {code}），请检查 API 密钥与配额，或稍后重试"),
    };
    Err(LlmCallError::from_http_api(code, err_text))
}

pub(super) fn boxed_non_stream_chat_parse_error(
    body: &str,
    parse_err: &serde_json::Error,
) -> Box<dyn std::error::Error + Send + Sync> {
    let preview = redact::single_line_preview(body, HTTP_BODY_PREVIEW_LOG_CHARS);
    error!(
        target: "crabmate",
        "非流式 chat 响应 JSON 解析失败 err={} body_len={} body_preview={}",
        parse_err,
        body.len(),
        preview
    );
    Box::<dyn std::error::Error + Send + Sync>::from("模型返回内容无法解析为预期格式，请稍后重试")
}
