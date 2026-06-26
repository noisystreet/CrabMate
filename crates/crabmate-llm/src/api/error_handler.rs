//! `chat/completions` HTTP 非成功体、非流式 JSON 解析失败等：日志与用户可见错误。

use log::error;

use crabmate_types::ChatRequest;

use crate::call_error::LlmCallError;
use crate::stream_host::StreamChatHost;

/// 在未开启 `RUST_LOG=…debug` 时，仍可用 **`CM_LOG_CHAT_REQUEST_JSON=1`** 在 **info** 级别打印请求体预览。
fn should_log_chat_request_json_preview() -> bool {
    log::log_enabled!(log::Level::Debug)
        || std::env::var_os("CM_LOG_CHAT_REQUEST_JSON").is_some_and(|v| {
            let s = v.to_string_lossy();
            let s = s.trim();
            !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
        })
}

pub(super) fn log_chat_request_json_preview_if_enabled(
    host: &dyn StreamChatHost,
    req: &ChatRequest,
) {
    if !should_log_chat_request_json_preview() {
        return;
    }
    host.log_chat_request_json_preview_if_enabled(req);
}

pub(super) async fn ensure_chat_completions_success(
    host: &dyn StreamChatHost,
    res: reqwest::Response,
) -> Result<reqwest::Response, LlmCallError> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    error!(
        target: "crabmate",
        "chat completions API 返回非成功状态 status={} body_len={}",
        status,
        body.len()
    );
    Err(host.llm_call_error_from_http_api(status.as_u16(), &body))
}

pub(super) fn boxed_non_stream_chat_parse_error(
    host: &dyn StreamChatHost,
    body: &str,
    parse_err: &serde_json::Error,
) -> Box<dyn std::error::Error + Send + Sync> {
    error!(
        target: "crabmate",
        "非流式 chat 响应 JSON 解析失败 err={} body_len={}",
        parse_err,
        body.len()
    );
    host.boxed_non_stream_chat_parse_error(body, parse_err)
}
