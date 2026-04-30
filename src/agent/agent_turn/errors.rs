//! 单轮 Agent 编排层错误：映射 **llm 层**失败为可观测子阶段（`sub_phase`）与用户/SSE 语义。

use std::error::Error;
use std::fmt;

use axum::http::StatusCode;

use crate::agent::per_coord::PerCoordinator;
use crate::llm::LlmCompleteError;
use crate::sse::SseErrorBody;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::LLM_CANCELLED_ERROR;
use crate::web::http_types::chat::ApiError;

/// 与 PER 命名对齐的观测子阶段（用于日志与 SSE 可选字段 `sub_phase`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnSubPhase {
    Planner,
    Executor,
    Reflect,
}

impl AgentTurnSubPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::Reflect => "reflect",
        }
    }
}

/// `plan_rewrite_exhausted` 控制面（与 `reason_code` 表一致）；带 `turn_id` / `sub_phase=reflect`。
pub(crate) fn sse_plan_rewrite_exhausted_body(
    tracing: Option<&std::sync::Arc<crate::observability::TracingChatTurn>>,
    reason: &str,
) -> SseErrorBody {
    SseErrorBody {
        error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
        code: Some("plan_rewrite_exhausted".to_string()),
        reason_code: Some(reason.to_string()),
        turn_id: tracing.map(|t| t.job_id),
        sub_phase: Some(AgentTurnSubPhase::Reflect.as_str().to_string()),
    }
}

/// SSE/客户端断开等导致的早停（非模型 HTTP 错误）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAbortReason {
    SseDisconnected,
    UserCancelled,
}

impl fmt::Display for TurnAbortReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SseDisconnected => f.write_str("流式输出已断开"),
            Self::UserCancelled => f.write_str(LLM_CANCELLED_ERROR),
        }
    }
}

/// `run_agent_turn` / `run_agent_turn_common` 失败时的结构化错误。
#[derive(Debug)]
pub enum RunAgentTurnError {
    /// 模型调用链失败（`complete_chat_retrying`）；**不**附带「第几步规划」等编排文案。
    Llm {
        phase: AgentTurnSubPhase,
        kind: LlmCompleteError,
    },
    /// 编排早停（如 SSE 已关闭仍尝试继续）。
    TurnAborted {
        phase: AgentTurnSubPhase,
        reason: TurnAbortReason,
    },
    /// 其它逻辑错误（如缺 `tool_calls`）；保留简短说明。
    Other {
        phase: AgentTurnSubPhase,
        message: String,
    },
    /// 步骤内重试次数耗尽
    StepRetryExhausted {
        phase: AgentTurnSubPhase,
        message: String,
    },
    /// 全局重规划次数耗尽
    ReplanExhausted {
        phase: AgentTurnSubPhase,
        message: String,
    },
    /// 墙钟超时
    TimeLimitExhausted {
        phase: AgentTurnSubPhase,
        message: String,
    },
    /// Token消耗上限达到
    TokenLimitExhausted {
        phase: AgentTurnSubPhase,
        message: String,
    },
}

impl RunAgentTurnError {
    /// 与 [`Self::sse_error_payload`] 顶层 `code` 一致；供 `POST /chat` JSON 等 HTTP 层复用。
    pub fn public_error_code(&self) -> &'static str {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => crate::types::SSE_STREAM_CANCELLED_CODE,
                LlmCompleteError::Transport(e) => {
                    if e.http_status == Some(429)
                        || crate::agent_errors::is_quota_or_rate_limit_llm_message(&e.user_message)
                    {
                        "LLM_RATE_LIMIT"
                    } else {
                        "LLM_REQUEST_FAILED"
                    }
                }
                LlmCompleteError::Other(_) => "INTERNAL_ERROR",
            },
            Self::TurnAborted { reason, .. } => match reason {
                TurnAbortReason::SseDisconnected => "turn_aborted",
                TurnAbortReason::UserCancelled => crate::types::SSE_STREAM_CANCELLED_CODE,
            },
            Self::Other { .. } => "INTERNAL_ERROR",
            Self::StepRetryExhausted { .. } => "STEP_RETRY_EXHAUSTED",
            Self::ReplanExhausted { .. } => "REPLAN_EXHAUSTED",
            Self::TimeLimitExhausted { .. } => "TIME_LIMIT_EXHAUSTED",
            Self::TokenLimitExhausted { .. } => "TOKEN_LIMIT_EXHAUSTED",
        }
    }

    /// 面向用户、可放入 HTTP `message` 或 SSE `error` 的短文案（**不含**内部排障细节）。
    pub fn public_user_message(&self) -> String {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => LLM_CANCELLED_ERROR.to_string(),
                LlmCompleteError::Transport(e) => e.user_message.clone(),
                LlmCompleteError::Other(_) => "对话失败，请稍后重试".to_string(),
            },
            Self::TurnAborted { reason, .. } => reason.to_string(),
            Self::Other { .. } => "对话失败，请稍后重试".to_string(),
            Self::StepRetryExhausted { .. } => {
                "本步重试次数已用尽，请简化任务或稍后重试".to_string()
            }
            Self::ReplanExhausted { .. } => "重规划次数已用尽，请简化任务或稍后重试".to_string(),
            Self::TimeLimitExhausted { .. } => "本轮对话已超时，请缩短输入或稍后重试".to_string(),
            Self::TokenLimitExhausted { .. } => {
                "本轮 token 预算已用尽，请缩短上下文或稍后重试".to_string()
            }
        }
    }

    /// HTTP 建议状态码（与 `ApiError` 同帧返回）；取消类为 **499**（非标准，表示客户端断开/取消）。
    pub fn suggested_http_status(&self) -> StatusCode {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => {
                    StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
                }
                LlmCompleteError::Transport(e) => {
                    if e.http_status == Some(429)
                        || crate::agent_errors::is_quota_or_rate_limit_llm_message(&e.user_message)
                    {
                        StatusCode::TOO_MANY_REQUESTS
                    } else if let Some(s) = e.http_status {
                        StatusCode::from_u16(s).unwrap_or(StatusCode::BAD_GATEWAY)
                    } else {
                        StatusCode::BAD_GATEWAY
                    }
                }
                LlmCompleteError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::TurnAborted { reason, .. } => match reason {
                TurnAbortReason::SseDisconnected => {
                    StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
                }
                TurnAbortReason::UserCancelled => {
                    StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
                }
            },
            Self::Other { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::StepRetryExhausted { .. }
            | Self::ReplanExhausted { .. }
            | Self::TimeLimitExhausted { .. }
            | Self::TokenLimitExhausted { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    /// `POST /chat` 等 JSON 错误体；**`reason_code`** 为截断后的内部摘要（若有），供排障与细粒度分支。
    pub(crate) fn http_api_error(&self) -> ApiError {
        if let Self::Llm {
            kind: LlmCompleteError::Transport(_),
            ..
        } = self
        {
            return ApiError::new(self.public_error_code(), self.public_user_message());
        }
        ApiError::with_reason(
            self.public_error_code(),
            self.public_user_message(),
            self.internal_reason_for_logs(),
        )
    }

    /// 写入 tracing 的短摘要（**可能**含编排细节；勿记录密钥）。
    pub fn internal_reason_for_logs(&self) -> String {
        truncate_chars_with_ellipsis(&self.to_string(), 240)
    }

    pub fn from_llm(phase: AgentTurnSubPhase, kind: LlmCompleteError) -> Self {
        Self::Llm { phase, kind }
    }

    pub fn sub_phase(&self) -> AgentTurnSubPhase {
        match self {
            Self::Llm { phase, .. }
            | Self::TurnAborted { phase, .. }
            | Self::StepRetryExhausted { phase, .. }
            | Self::ReplanExhausted { phase, .. }
            | Self::TimeLimitExhausted { phase, .. }
            | Self::TokenLimitExhausted { phase, .. }
            | Self::Other { phase, .. } => *phase,
        }
    }

    /// Web `job_id`（与 `x-stream-job-id` / `sse_capabilities` 一致）；CLI 等为 `None`。
    pub fn turn_id(
        tracing: Option<&std::sync::Arc<crate::observability::TracingChatTurn>>,
    ) -> Option<u64> {
        tracing.map(|t| t.job_id)
    }

    pub fn sse_error_payload(&self, turn_id: Option<u64>) -> SseErrorBody {
        let sub_phase = Some(self.sub_phase().as_str().to_string());
        match self {
            Self::Llm { kind, .. } => {
                let (code, user_msg, reason_code) = match kind {
                    LlmCompleteError::Cancelled => (
                        crate::types::SSE_STREAM_CANCELLED_CODE.to_string(),
                        LLM_CANCELLED_ERROR.to_string(),
                        None,
                    ),
                    LlmCompleteError::Transport(e) => {
                        let code = if e.http_status == Some(429)
                            || crate::agent_errors::is_quota_or_rate_limit_llm_message(
                                &e.user_message,
                            ) {
                            "LLM_RATE_LIMIT".to_string()
                        } else {
                            "LLM_REQUEST_FAILED".to_string()
                        };
                        (code, e.user_message.clone(), None)
                    }
                    LlmCompleteError::Other(e) => (
                        "INTERNAL_ERROR".to_string(),
                        "对话失败，请稍后重试".to_string(),
                        Some(truncate_reason(e.to_string())),
                    ),
                };
                SseErrorBody {
                    error: user_msg,
                    code: Some(code),
                    reason_code,
                    turn_id,
                    sub_phase,
                }
            }
            Self::TurnAborted { reason, .. } => {
                let (code, msg) = match reason {
                    TurnAbortReason::SseDisconnected => {
                        ("turn_aborted".to_string(), reason.to_string())
                    }
                    TurnAbortReason::UserCancelled => (
                        crate::types::SSE_STREAM_CANCELLED_CODE.to_string(),
                        reason.to_string(),
                    ),
                };
                SseErrorBody {
                    error: msg,
                    code: Some(code),
                    reason_code: None,
                    turn_id,
                    sub_phase,
                }
            }
            Self::Other { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("INTERNAL_ERROR".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
            },
            Self::StepRetryExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("STEP_RETRY_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
            },
            Self::ReplanExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("REPLAN_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
            },
            Self::TimeLimitExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("TIME_LIMIT_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
            },
            Self::TokenLimitExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("TOKEN_LIMIT_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
            },
        }
    }

    /// 协作取消或用户显式取消：与 `cancel` 标志、`LLM_CANCELLED_ERROR` 对齐。
    pub(crate) fn is_user_flow_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Llm {
                kind: LlmCompleteError::Cancelled,
                ..
            } | Self::TurnAborted {
                reason: TurnAbortReason::UserCancelled,
                ..
            }
        )
    }

    /// `GET /status` 等任务摘要用短串（取消路径返回 `None`）。
    pub(crate) fn short_detail_for_job_log(&self) -> Option<String> {
        if self.is_user_flow_cancelled() {
            return None;
        }
        Some(truncate_chars_with_ellipsis(&self.to_string(), 120))
    }
}

fn truncate_reason(s: String) -> String {
    const MAX: usize = 200;
    if s.chars().count() <= MAX {
        return s;
    }
    let mut out: String = s.chars().take(MAX.saturating_sub(1)).collect();
    out.push('…');
    out
}

impl fmt::Display for RunAgentTurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Llm { kind, .. } => write!(f, "{kind}"),
            Self::TurnAborted { reason, .. } => write!(f, "{reason}"),
            Self::Other { message, .. }
            | Self::StepRetryExhausted { message, .. }
            | Self::ReplanExhausted { message, .. }
            | Self::TimeLimitExhausted { message, .. }
            | Self::TokenLimitExhausted { message, .. } => f.write_str(message),
        }
    }
}

impl Error for RunAgentTurnError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Llm { kind, .. } => Some(kind),
            Self::TurnAborted { .. }
            | Self::StepRetryExhausted { .. }
            | Self::ReplanExhausted { .. }
            | Self::TimeLimitExhausted { .. }
            | Self::TokenLimitExhausted { .. }
            | Self::Other { .. } => None,
        }
    }
}

/// 分阶段规划等路径中，规划轮 LLM 失败统一记为 **`sub_phase=planner`**。
impl From<LlmCompleteError> for RunAgentTurnError {
    fn from(value: LlmCompleteError) -> Self {
        Self::from_llm(AgentTurnSubPhase::Planner, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_sse_uses_public_message_and_reason_detail() {
        let e = RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "internal: missing tool_calls".to_string(),
        };
        let sse = e.sse_error_payload(Some(7));
        assert_eq!(sse.code.as_deref(), Some("INTERNAL_ERROR"));
        assert_eq!(sse.error, "对话失败，请稍后重试");
        assert!(
            sse.reason_code
                .as_deref()
                .is_some_and(|s| s.contains("missing"))
        );
        assert_eq!(sse.sub_phase.as_deref(), Some("executor"));
        assert_eq!(sse.turn_id, Some(7));
    }

    #[test]
    fn step_retry_maps_to_422_and_distinct_code() {
        let e = RunAgentTurnError::StepRetryExhausted {
            phase: AgentTurnSubPhase::Planner,
            message: "step failed".to_string(),
        };
        assert_eq!(e.public_error_code(), "STEP_RETRY_EXHAUSTED");
        assert_eq!(e.suggested_http_status(), StatusCode::UNPROCESSABLE_ENTITY);
        let api = e.http_api_error();
        assert_eq!(api.code, "STEP_RETRY_EXHAUSTED");
        assert!(api.reason_code.is_some());
    }
}
