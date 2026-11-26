//! 单轮回合的**端口错误类型**与纯判定（#1510「结论类型与纯判定上提」）。
//!
//! 归属此处而非 `agent::agent_turn::host::errors`：`TurnEngine::run` 的签名是
//! `Result<TurnOutcome, RunAgentTurnError>` —— 错误类型属**端口契约**。把定义留在 `agent`
//! 里，`turn_runner` 就不得不反向依赖实现层，端口层与实现层之间因此多一条倒置边。
//!
//! 本模块只收**不含 Web/HTTP 语义**的部分：变体、`Display` / `Error`、对外 `code` 与用户
//! 文案、队列分流判定、日志摘要。`axum` 状态码 / `ApiError` / `SseErrorBody` 的映射留在
//! `crate::agent::agent_turn::host::errors` —— 那是「怎么表达给客户端」，不是「错误是什么」。
//!
//! 依赖方向：`cm_agent` / `cm_llm` / `cm_types` / `text_util`，**不依赖** `agent`。

use std::error::Error;
use std::fmt;

use crate::cm_agent::turn_budget::TurnBudgetDeny;
use crate::cm_llm::LlmCompleteError;
use crate::cm_types::LLM_CANCELLED_ERROR;
use crate::text_util::truncate_chars_with_ellipsis;

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
    /// 单轮预算耗尽（墙钟 / LLM 调用次数 / Token 粗估）。
    ///
    /// 用**一个**变体携带 [`TurnBudgetDeny`]，而不是每种原因一个变体：新增预算维度时只需给
    /// [`TurnBudgetDeny`] 加变体，本枚举的穷举 match 不必各加一臂；`delegation` 侧只要 `..`
    /// 就能覆盖（子任务视角只有「超预算」一种语义）。
    TurnBudgetExhausted {
        phase: AgentTurnSubPhase,
        deny: TurnBudgetDeny,
        /// 面向用户的短消息（构造点为 `deny.user_message(cfg)`，含具体数值）。
        message: String,
    },
}

/// Web 队列 worker 对 [`RunAgentTurnError`] 的**分流种类**（与 `chat_job_queue` 中 `Err` 分支一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentTurnJobOutcomeKind {
    /// 用户/协作取消（含 `Display` 串上的取消启发式）。
    UserCancelled,
    /// 发送 **`sse_error_payload`** 的失败路径。
    FailureEmitSseError,
}

impl RunAgentTurnError {
    /// 非流式 JSON 任务：`Err` 分支与 [`Self::job_queue_stream_outcome_kind`] 共用核心归类（无 `cancelled_by_signal`）。
    pub(crate) fn job_queue_json_outcome_kind(&self) -> AgentTurnJobOutcomeKind {
        self.job_queue_outcome_kind_inner()
    }

    /// 流式任务：`cancelled_by_signal` 优先；否则与 JSON 共用归类。
    pub(crate) fn job_queue_stream_outcome_kind(
        &self,
        cancelled_by_signal: bool,
    ) -> AgentTurnJobOutcomeKind {
        if cancelled_by_signal {
            AgentTurnJobOutcomeKind::UserCancelled
        } else {
            self.job_queue_outcome_kind_inner()
        }
    }

    /// 只按**变体**判定（[`Self::is_user_flow_cancelled`]），不把 `Display` 文案丢进字符串比对（#1478）。
    ///
    /// 原来还多一条 `is_user_cancelled_run_agent_error(&self.to_string())` 兜底：它要求
    /// `Other { message }` 恰好等于取消串，而生产代码从不那样构造（只有测试会），
    /// 既是死代码、又会随文案改动静默失效。
    fn job_queue_outcome_kind_inner(&self) -> AgentTurnJobOutcomeKind {
        if self.is_user_flow_cancelled() {
            AgentTurnJobOutcomeKind::UserCancelled
        } else {
            AgentTurnJobOutcomeKind::FailureEmitSseError
        }
    }

    /// 与 SSE 顶层 `code` 一致；供 `POST /chat` JSON 等 HTTP 层复用。
    pub fn public_error_code(&self) -> &'static str {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => crate::cm_types::SSE_STREAM_CANCELLED_CODE,
                // #1440 K3：配额判定在 `cm_llm::classify_llm_failure`（构造点）完成，这里只读变体。
                LlmCompleteError::RateLimit(_) => "LLM_RATE_LIMIT",
                LlmCompleteError::Transport(_) => "LLM_REQUEST_FAILED",
                LlmCompleteError::Other(_) => "INTERNAL_ERROR",
            },
            Self::TurnAborted { reason, .. } => match reason {
                TurnAbortReason::SseDisconnected => "turn_aborted",
                TurnAbortReason::UserCancelled => crate::cm_types::SSE_STREAM_CANCELLED_CODE,
            },
            Self::Other { .. } => "INTERNAL_ERROR",
            Self::StepRetryExhausted { .. } => "STEP_RETRY_EXHAUSTED",
            Self::ReplanExhausted { .. } => "REPLAN_EXHAUSTED",
            Self::TurnBudgetExhausted { deny, .. } => budget_deny_error_code(*deny),
        }
    }

    /// 面向用户、可放入 HTTP `message` 或 SSE `error` 的短文案（**不含**内部排障细节）。
    pub fn public_user_message(&self) -> String {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => LLM_CANCELLED_ERROR.to_string(),
                LlmCompleteError::Transport(e) | LlmCompleteError::RateLimit(e) => {
                    e.user_message.clone()
                }
                LlmCompleteError::Other(_) => "对话失败，请稍后重试".to_string(),
            },
            Self::TurnAborted { reason, .. } => reason.to_string(),
            Self::Other { .. } => "对话失败，请稍后重试".to_string(),
            Self::StepRetryExhausted { .. } => {
                "本步重试次数已用尽，请简化任务或稍后重试".to_string()
            }
            Self::ReplanExhausted { .. } => "重规划次数已用尽，请简化任务或稍后重试".to_string(),
            Self::TurnBudgetExhausted { deny, .. } => match deny {
                TurnBudgetDeny::WallClock => "本轮对话已超时，请缩短输入或稍后重试",
                TurnBudgetDeny::LlmCalls => "本轮模型调用次数已用尽，请简化任务或稍后重试",
                TurnBudgetDeny::Tokens => "本轮 token 预算已用尽，请缩短上下文或稍后重试",
            }
            .to_string(),
        }
    }

    /// 单行键值摘要，供 **`tracing`** 与队列 worker 统一拼接（**勿**记录密钥）。
    pub fn diag_log_kv(&self) -> String {
        format!(
            "public_code={} sub_phase={} detail={}",
            self.public_error_code(),
            self.sub_phase().as_str(),
            self.internal_reason_for_logs()
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
            | Self::TurnBudgetExhausted { phase, .. }
            | Self::Other { phase, .. } => *phase,
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

    /// 中断类取消（含 SSE 断开）：用于判断「本轮已产出的部分是否要写回 canonical」。
    ///
    /// 与 [`Self::is_user_flow_cancelled`] 的差异：后者只覆盖 `UserCancelled`（SseDisconnected
    /// 不算用户取消，故不用于落盘判定）；但 `check_abort!` 在 SSE 已关闭时**优先**归为
    /// `SseDisconnected`（即便 `cancel` 已置位），而 job 队列据 `cancelled_by_signal` 仍会把它
    /// 归为 `UserCancelled` —— 因此落盘判定必须同时覆盖 `SseDisconnected`，否则该轮部分输出
    /// 会在「用户点停止 → SSE 先断开」的竞态路径上丢失。
    pub(crate) fn is_interrupted_cancel(&self) -> bool {
        self.is_user_flow_cancelled()
            || matches!(
                self,
                Self::TurnAborted {
                    reason: TurnAbortReason::SseDisconnected,
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

/// 预算原因 → 对外 `code`。HTTP 与 SSE 共用同一份映射，避免两处漂移。
pub(crate) const fn budget_deny_error_code(deny: TurnBudgetDeny) -> &'static str {
    match deny {
        TurnBudgetDeny::WallClock => "TIME_LIMIT_EXHAUSTED",
        TurnBudgetDeny::LlmCalls => "LLM_CALL_LIMIT_EXHAUSTED",
        TurnBudgetDeny::Tokens => "TOKEN_LIMIT_EXHAUSTED",
    }
}

impl fmt::Display for RunAgentTurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Llm { kind, .. } => write!(f, "{kind}"),
            Self::TurnAborted { reason, .. } => write!(f, "{reason}"),
            Self::Other { message, .. }
            | Self::StepRetryExhausted { message, .. }
            | Self::ReplanExhausted { message, .. }
            | Self::TurnBudgetExhausted { message, .. } => f.write_str(message),
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
            | Self::TurnBudgetExhausted { .. }
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

    /// 反向守卫（#1478）：`Display` 文案**不参与**分流判定。
    ///
    /// 改动前这里断言「`Other { message: 取消串 }` 被当作取消」——那正是文案启发式；生产代码
    /// 从不这样构造，删掉兜底后该错误正确落回 `FailureEmitSseError`，而结构化取消路径照常命中。
    #[test]
    fn job_queue_outcome_kind_ignores_display_text() {
        let text_only = RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: LLM_CANCELLED_ERROR.to_string(),
        };
        assert_eq!(
            text_only.job_queue_json_outcome_kind(),
            AgentTurnJobOutcomeKind::FailureEmitSseError,
            "文案不参与判定"
        );

        let cancelled = RunAgentTurnError::TurnAborted {
            phase: AgentTurnSubPhase::Planner,
            reason: TurnAbortReason::UserCancelled,
        };
        assert_eq!(
            cancelled.job_queue_json_outcome_kind(),
            AgentTurnJobOutcomeKind::UserCancelled,
            "结构化取消路径仍然命中"
        );
    }

    /// 纯判定的 `code` / `sub_phase` / `detail` 三键稳定（队列 worker 与 tracing 都按它拼串）。
    #[test]
    fn diag_log_kv_contains_stable_keys() {
        let e = RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Reflect,
            message: "x".into(),
        };
        let s = e.diag_log_kv();
        assert!(s.contains("public_code=INTERNAL_ERROR"));
        assert!(s.contains("sub_phase=reflect"));
        assert!(s.contains("detail="));
    }
}
