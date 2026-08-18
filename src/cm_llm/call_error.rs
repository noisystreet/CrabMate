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

    /// `reqwest` 在拿到响应前后失败：**超时**、**连接建立失败** 与 **发送阶段失败**（`Kind::Request`，
    /// 含 hyper `SendRequest`：请求未发送完成即连接被关闭/重置，对端未产生响应，重发同一请求安全）
    /// 视为可重试；其余（如 TLS 校验、响应解析 `Decode`、响应体读取 `Body` 等）默认不重试以免放大问题。
    pub fn boxed_from_reqwest(e: reqwest::Error) -> Box<dyn Error + Send + Sync> {
        let retryable = reqwest_transport_error_retryable(&e);
        let msg = crate::cm_llm::http_client::format_reqwest_transport_err(&e);
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

/// `reqwest` 传输层错误是否可退避重试（[`LlmCallError::boxed_from_reqwest`] 与日志共用）。
///
/// - **可重试**：`is_timeout()`（超时）、`is_connect()`（连接建立失败），以及 `is_request()`
///   （发送阶段失败，如 hyper `SendRequest`：连接建立后、响应到达前被关闭/重置——典型的瞬态
///   网络故障，对端未产生响应，重发同一请求安全）。
/// - **不可重试**：其余（TLS 校验、响应解析、响应体读取等），默认不重试以免放大问题。
pub fn reqwest_transport_error_retryable(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request()
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

    /// 真实构造 hyper `SendRequest` 类错误（回环服务端接受连接后立即关闭，不读取不响应）：
    /// 回归验证「发送阶段失败」必须可重试——否则一次连接抖动（对端无响应）就会让整轮 LLM
    /// 调用在 `attempt=1` 直接失败，尽管配置了多次重试。
    #[test]
    fn send_request_kind_error_is_retryable() {
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .unwrap();
                tokio::spawn(async move {
                    // 接受连接后立即 drop（发 FIN、不响应），客户端在发送/读取阶段失败。
                    let _ = listener.accept().await;
                });
                client
                    .get(format!("http://{addr}/"))
                    .send()
                    .await
                    .expect_err("连接被立即关闭，请求应当失败")
            });
        assert!(err.is_request(), "错误应为发送阶段失败（Kind::Request）: {err}");
        assert!(!err.is_connect(), "连接已建立，不应归类为连接失败: {err}");
        assert!(
            reqwest_transport_error_retryable(&err),
            "SendRequest 类错误必须可重试: {err}"
        );
        let boxed = LlmCallError::boxed_from_reqwest(err);
        assert!(
            llm_call_error_retryable(boxed.as_ref()),
            "boxed_from_reqwest 应保留 retryable=true"
        );
    }

    /// 回环无监听端口触发连接拒绝（`is_connect`），也应可重试（既有行为回归）。
    #[test]
    fn connect_refused_error_is_retryable() {
        let err = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let port = {
                let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                l.local_addr().unwrap().port()
            };
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            client
                .get(format!("http://127.0.0.1:{port}/"))
                .send()
                .await
                .expect_err("无监听端口，连接应被拒绝")
        });
        assert!(err.is_connect(), "{err}");
        assert!(reqwest_transport_error_retryable(&err));
    }
}
