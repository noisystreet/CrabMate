//! [`super::complete_chat_retrying`] 的失败类型：在 **llm 层**聚合为结构化变体（含可重试与 HTTP 状态），**不含** agent 编排语义（如「第几步规划」）。

use std::error::Error;
use std::fmt;

use crate::cm_types::LLM_CANCELLED_ERROR;

use super::call_error::{LlmCallError, LlmFailureClass};

/// `complete_chat_retrying` 的最终失败（成功路径外的所有情况）。
#[derive(Debug)]
pub enum LlmCompleteError {
    /// 用户/协作取消（与 [`LLM_CANCELLED_ERROR`] 对齐）。
    Cancelled,
    /// 传输或 HTTP 层失败（含脱敏后的 [`LlmCallError::user_message`]）。
    Transport(LlmCallError),
    /// 配额 / 限流（`429` / `402`，或响应体带明确配额语义）。
    ///
    /// 与 [`Self::Transport`] 载荷相同，差别只在**分类**：由 [`LlmCallError::class`] 在构造点
    /// 一次判定（#1440 K3）。下游据此直接映射 `LLM_RATE_LIMIT` / `429`，**不再**对文案做子串反查。
    RateLimit(LlmCallError),
    /// 其它失败（解析、逻辑分支等），保留 `source` 供日志。
    Other(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for LlmCompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str(LLM_CANCELLED_ERROR),
            Self::Transport(e) | Self::RateLimit(e) => f.write_str(&e.user_message),
            Self::Other(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl Error for LlmCompleteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled => None,
            Self::Transport(e) | Self::RateLimit(e) => Some(e),
            Self::Other(e) => Some(e.as_ref()),
        }
    }
}

impl LlmCompleteError {
    pub fn from_boxed(e: Box<dyn Error + Send + Sync>) -> Self {
        if e.to_string().trim() == LLM_CANCELLED_ERROR {
            return Self::Cancelled;
        }
        if let Some(llm) = e.downcast_ref::<LlmCallError>() {
            // 结构化分流：分类在 `LlmCallError` 构造点已完成，这里只做枚举映射。
            return match llm.class {
                LlmFailureClass::QuotaOrRateLimit => Self::RateLimit(llm.clone()),
                LlmFailureClass::Other => Self::Transport(llm.clone()),
            };
        }
        Self::Other(e)
    }

    /// 是否属于配额 / 限流类（**结构化**，不读文案）。
    ///
    /// 刻意**不**叫 `is_quota_or_rate_limit_llm_message` —— 那是 `agent_errors` 里的
    /// **字符串反查**（只服务 CLI 退出码），两者不是同一判据。
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit(_))
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Cancelled => false,
            Self::Transport(e) | Self::RateLimit(e) => e.retryable,
            Self::Other(_) => false,
        }
    }

    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::Cancelled => None,
            Self::Transport(e) | Self::RateLimit(e) => e.http_status,
            Self::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_string_matches_constant() {
        assert_eq!(LlmCompleteError::Cancelled.to_string(), LLM_CANCELLED_ERROR);
    }

    /// #1440 K3：`LlmCallError::class` 决定分到 `RateLimit` 还是 `Transport`（下游零文案反查）。
    #[test]
    fn from_boxed_routes_by_structured_class() {
        let limited: Box<dyn Error + Send + Sync> =
            Box::new(LlmCallError::from_http_api(429, "rate limited".to_string()));
        assert!(
            LlmCompleteError::from_boxed(limited).is_rate_limited(),
            "429 应分到 RateLimit"
        );

        // 上游 503 的真实文案恒含「（HTTP 503）」——不得被判成限流。
        let unavail: Box<dyn Error + Send + Sync> = Box::new(LlmCallError::from_http_api(
            503,
            "模型接口返回错误（HTTP 503），请检查 API 密钥与配额，或稍后重试".to_string(),
        ));
        let got = LlmCompleteError::from_boxed(unavail);
        assert!(!got.is_rate_limited(), "503 不应是限流");
        assert_eq!(got.http_status(), Some(503));
    }
}
