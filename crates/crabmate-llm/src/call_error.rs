//! `chat/completions` 调用错误：携带 **HTTP 状态**（若有）与 **是否参与退避重试**，供 [`super::complete_chat_retrying`] 与日志对齐。

use std::error::Error;
use std::fmt;

/// 模型 HTTP 调用失败（含传输层）；与 `redact` 后的用户可见文案一致，并标记是否应指数退避重试。
#[derive(Debug, Clone)]
pub struct LlmCallError {
    /// 已脱敏、可展示给 CLI/Web 的说明（与历史 `String` 错误串形状对齐）。
    pub user_message: String,
    pub http_status: Option<u16>,
    pub retryable: bool,
}

impl fmt::Display for LlmCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message)
    }
}

impl Error for LlmCallError {}

/// 按 HTTP 状态判断是否应对**同一请求体**做退避重试（与 OpenAI 兼容网关常见语义对齐）。
///
/// - **可重试**：`408`、`429`、**5xx**（对端/网关瞬时故障或限流）。
/// - **不可重试**：**4xx** 其余（含 `400` 参数错误、`401`/`403` 鉴权、`404` 路径等），重试通常浪费配额且不改变结果。
pub fn http_status_retryable_for_backoff(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

impl LlmCallError {
    pub fn from_http_api(status: u16, user_message: String) -> Self {
        Self {
            retryable: http_status_retryable_for_backoff(status),
            http_status: Some(status),
            user_message,
        }
    }

    /// `reqwest` 在拿到响应前后失败：仅 **超时** 与 **连接建立失败** 视为可重试，其余（如 TLS 校验、解析）默认不重试以免放大问题。
    pub fn boxed_from_reqwest(e: reqwest::Error) -> Box<dyn Error + Send + Sync> {
        let retryable = e.is_timeout() || e.is_connect();
        let msg = crate::http_client::format_reqwest_transport_err(&e);
        Box::new(Self {
            user_message: msg,
            http_status: None,
            retryable,
        })
    }
}

pub fn llm_call_error_retryable(e: &(dyn Error + Send + Sync + 'static)) -> bool {
    e.downcast_ref::<LlmCallError>()
        .is_some_and(|x| x.retryable)
}

pub fn llm_call_error_http_status(e: &(dyn Error + Send + Sync + 'static)) -> Option<u16> {
    e.downcast_ref::<LlmCallError>().and_then(|x| x.http_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_retryable_matches_table() {
        assert!(!http_status_retryable_for_backoff(400));
        assert!(!http_status_retryable_for_backoff(401));
        assert!(!http_status_retryable_for_backoff(403));
        assert!(!http_status_retryable_for_backoff(404));
        assert!(http_status_retryable_for_backoff(408));
        assert!(http_status_retryable_for_backoff(429));
        assert!(http_status_retryable_for_backoff(500));
        assert!(http_status_retryable_for_backoff(503));
        assert!(http_status_retryable_for_backoff(599));
        assert!(!http_status_retryable_for_backoff(600));
    }

    #[test]
    fn display_is_user_message() {
        let e = LlmCallError::from_http_api(401, "模型接口返回错误（HTTP 401）：x".to_string());
        assert_eq!(e.to_string(), "模型接口返回错误（HTTP 401）：x");
        assert!(!e.retryable);
    }
}
