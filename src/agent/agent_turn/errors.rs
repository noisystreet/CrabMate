//! 单轮 Agent 编排层错误：映射 **llm 层**失败为可观测子阶段（`sub_phase`）与用户/SSE 语义。

use std::error::Error;
use std::fmt;

use crate::agent::per_coord::PerCoordinator;
use crate::llm::LlmCompleteError;
use crate::sse::SseErrorBody;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::LLM_CANCELLED_ERROR;

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
                error: message.clone(),
                code: Some("INTERNAL_ERROR".to_string()),
                reason_code: None,
                turn_id,
                sub_phase,
            },
            Self::StepRetryExhausted { message, .. } => SseErrorBody {
                error: message.clone(),
                code: Some("STEP_RETRY_EXHAUSTED".to_string()),
                reason_code: Some("step_retry_exhausted".to_string()),
                turn_id,
                sub_phase,
            },
            Self::ReplanExhausted { message, .. } => SseErrorBody {
                error: message.clone(),
                code: Some("REPLAN_EXHAUSTED".to_string()),
                reason_code: Some("replan_exhausted".to_string()),
                turn_id,
                sub_phase,
            },
            Self::TimeLimitExhausted { message, .. } => SseErrorBody {
                error: message.clone(),
                code: Some("TIME_LIMIT_EXHAUSTED".to_string()),
                reason_code: Some("time_limit_exhausted".to_string()),
                turn_id,
                sub_phase,
            },
            Self::TokenLimitExhausted { message, .. } => SseErrorBody {
                error: message.clone(),
                code: Some("TOKEN_LIMIT_EXHAUSTED".to_string()),
                reason_code: Some("token_limit_exhausted".to_string()),
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
