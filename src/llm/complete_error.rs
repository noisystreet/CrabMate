//! [`super::complete_chat_retrying`] 的失败类型：在 **llm 层**聚合为结构化变体（含可重试与 HTTP 状态），**不含** agent 编排语义（如「第几步规划」）。

use std::error::Error;
use std::fmt;

use crate::types::LLM_CANCELLED_ERROR;

use super::call_error::LlmCallError;

/// `complete_chat_retrying` 的最终失败（成功路径外的所有情况）。
#[derive(Debug)]
pub enum LlmCompleteError {
    /// 用户/协作取消（与 [`LLM_CANCELLED_ERROR`] 对齐）。
    Cancelled,
    /// 传输或 HTTP 层失败（含脱敏后的 [`LlmCallError::user_message`]）。
    Transport(LlmCallError),
    /// 其它失败（解析、逻辑分支等），保留 `source` 供日志。
    Other(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for LlmCompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str(LLM_CANCELLED_ERROR),
            Self::Transport(e) => f.write_str(&e.user_message),
            Self::Other(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl Error for LlmCompleteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled => None,
            Self::Transport(e) => Some(e),
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
            return Self::Transport(llm.clone());
        }
        Self::Other(e)
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Cancelled => false,
            Self::Transport(e) => e.retryable,
            Self::Other(_) => false,
        }
    }

    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::Cancelled => None,
            Self::Transport(e) => e.http_status,
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
}
