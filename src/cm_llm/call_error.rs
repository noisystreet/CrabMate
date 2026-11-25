//! `chat/completions` 调用错误：携带 **HTTP 状态**（若有）与 **是否参与退避重试**，供 [`super::complete_chat_retrying`] 与日志对齐。

use std::error::Error;
use std::fmt;

/// 一次 LLM HTTP / 传输失败的**结构化分类**。
///
/// 判定只在构造点（[`LlmCallError::from_http_api`] / [`LlmCallError::boxed_from_reqwest`]）做一次；
/// 下游（SSE `code`、HTTP 状态、CLI 退出码）只读本字段，**不再**对用户可见文案做子串反查（#1440 K3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmFailureClass {
    /// 配额 / 限流：`429`、`402`，或响应体带明确配额语义（部分网关用 `400` 报 quota）。
    QuotaOrRateLimit,
    /// 其余 HTTP / 传输失败。
    Other,
}

/// 响应体中**明确表示配额 / 限流**的语义片段（上游文案，与状态码互补）。
///
/// 刻意**不含** `HTTP 429` / `status=429` / `HTTP 503` 这类状态片段：状态已由
/// [`classify_llm_failure`] 的结构化分支处理，留在文本里只会在状态本非配额时制造误判——
/// 历史实现因此把上游 503 误判成限流（#1440 K3）。
///
/// **不要把中文「配额」加进来**：`LlmCallError::from_http_api` 的兜底 `user_message`
/// 恒为「…请检查 API 密钥与**配额**，或稍后重试」，一旦命中，所有 5xx 都会变成配额类。
/// 生产路径已改读**原始响应体**（见 [`LlmCallError::from_http_api_classified`]），但便捷构造
/// 仍可能把该文案传进来。
const QUOTA_SEMANTIC_NEEDLES: &[&str] = &[
    "quota",
    "Quota",
    "insufficient",
    "rate limit",
    "too many requests",
    "限流",
    "余额",
];

/// 鉴权 / 路径类状态：**短路**语义反查，避免 `账户余额充足` 这类正文把鉴权失败判成配额。
fn status_is_auth_or_route(status: u16) -> bool {
    matches!(status, 401 | 403 | 404)
}

/// 结构化判定一次：状态码优先，其次响应体语义片段。
///
/// `text` 传**上游响应体摘要**（`user_message` 亦可，其内容是 `（HTTP {status}）：{摘要}`）。
pub fn classify_llm_failure(status: Option<u16>, text: &str) -> LlmFailureClass {
    if let Some(s) = status {
        if matches!(s, 429 | 402) {
            return LlmFailureClass::QuotaOrRateLimit;
        }
        if status_is_auth_or_route(s) {
            return LlmFailureClass::Other;
        }
    }
    if QUOTA_SEMANTIC_NEEDLES.iter().any(|n| text.contains(n)) {
        LlmFailureClass::QuotaOrRateLimit
    } else {
        LlmFailureClass::Other
    }
}

/// 模型 HTTP 调用失败（含传输层）；与 `redact` 后的用户可见文案一致，并标记是否应指数退避重试。
#[derive(Debug, Clone)]
pub struct LlmCallError {
    /// 已脱敏、可展示给 CLI/Web 的说明（与历史 `String` 错误串形状对齐）。
    pub user_message: String,
    pub http_status: Option<u16>,
    pub retryable: bool,
    /// 配额 / 限流与否（构造点一次判定，见 [`classify_llm_failure`]）。
    pub class: LlmFailureClass,
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
    /// **生产入口**：分类读**原始响应体**（而非拼接后的 `user_message`），
    /// 这样「兜底提示语的措辞」与「判定」彻底解耦。
    pub fn from_http_api_classified(status: u16, user_message: String, body: &str) -> Self {
        Self {
            retryable: http_status_retryable_for_backoff(status),
            http_status: Some(status),
            class: classify_llm_failure(Some(status), body),
            user_message,
        }
    }

    /// 便捷构造：分类只在 `user_message` 上做（测试 / golden / 无独立 body 的调用点）。
    ///
    /// 生产路径请用 [`Self::from_http_api_classified`]——`user_message` 里混着
    /// 「请检查 API 密钥与配额」这类**我们自己写的**提示语，不该参与语义判定。
    pub fn from_http_api(status: u16, user_message: String) -> Self {
        Self {
            retryable: http_status_retryable_for_backoff(status),
            http_status: Some(status),
            class: classify_llm_failure(Some(status), &user_message),
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
            class: LlmFailureClass::Other,
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
        let err = tokio::runtime::Runtime::new().unwrap().block_on(async {
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
        assert!(
            err.is_request(),
            "错误应为发送阶段失败（Kind::Request）: {err}"
        );
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

    /// #1440 K3：配额判定只做在**构造点**，且状态码优先于文本片段。
    ///
    /// 关键回归：上游 `503` 的 `user_message` 恒含 `（HTTP 503）`，历史实现靠子串反查把它
    /// 误判成限流（对外 `429` / `LLM_RATE_LIMIT`）；结构化后必须回落成普通失败。
    #[test]
    fn classify_llm_failure_prefers_status_and_ignores_status_fragments() {
        let unavail = LlmCallError::from_http_api(
            503,
            "模型接口返回错误（HTTP 503），请检查 API 密钥与配额，或稍后重试".to_string(),
        );
        assert_eq!(unavail.class, LlmFailureClass::Other, "503 不是限流");

        let limited =
            LlmCallError::from_http_api(429, "模型接口返回错误（HTTP 429）：x".to_string());
        assert_eq!(limited.class, LlmFailureClass::QuotaOrRateLimit);

        let payment = LlmCallError::from_http_api(402, "payment required".to_string());
        assert_eq!(payment.class, LlmFailureClass::QuotaOrRateLimit);

        // 非标网关：用 400 报配额（golden `llm_transport_quota_msg` 的契约）。
        let quota_400 =
            LlmCallError::from_http_api(400, "You exceeded your current quota".to_string());
        assert_eq!(quota_400.class, LlmFailureClass::QuotaOrRateLimit);

        // 鉴权类短路：正文里的「余额充足」不得把 401 判成配额。
        let auth = LlmCallError::from_http_api(
            401,
            "模型接口返回错误（HTTP 401）：账户余额充足".to_string(),
        );
        assert_eq!(auth.class, LlmFailureClass::Other);

        // 无响应（传输层）一律非配额。
        assert_eq!(
            classify_llm_failure(None, "connection reset"),
            LlmFailureClass::Other
        );
    }

    /// 生产入口只读**原始响应体**：`user_message` 里的自有提示语不参与判定。
    #[test]
    fn classified_constructor_reads_body_not_user_message() {
        // body 无配额语义 → 503 仍是普通失败，哪怕 user_message 里有「配额」二字。
        let unavail = LlmCallError::from_http_api_classified(
            503,
            "模型接口返回错误（HTTP 503），请检查 API 密钥与配额，或稍后重试".to_string(),
            "<html>502 Bad Gateway</html>",
        );
        assert_eq!(unavail.class, LlmFailureClass::Other);
        assert_eq!(unavail.http_status, Some(503));

        // body 有配额语义 → 判配额。
        let quota = LlmCallError::from_http_api_classified(
            500,
            "模型接口返回错误（HTTP 500），请检查 API 密钥与配额，或稍后重试".to_string(),
            "{\"error\":{\"message\":\"insufficient_quota\"}}",
        );
        assert_eq!(quota.class, LlmFailureClass::QuotaOrRateLimit);

        // 英文限流短语（网关常见形态）。
        assert_eq!(
            classify_llm_failure(Some(503), "rate limit exceeded, please retry later"),
            LlmFailureClass::QuotaOrRateLimit
        );
    }
}
