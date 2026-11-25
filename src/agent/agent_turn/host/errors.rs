//! 单轮 Agent 编排层错误：映射 **llm 层**失败为可观测子阶段（`sub_phase`）与用户/SSE 语义。
//!
//! **`chat_job_queue`** 对 `Err(RunAgentTurnError)` 的分流见 **[`AgentTurnJobOutcomeKind`]** 与
//! **[`RunAgentTurnError::job_queue_stream_outcome_kind`] / [`RunAgentTurnError::job_queue_json_outcome_kind`]**
//!（避免 worker 内重复的字符串前缀判断）。

use std::error::Error;
use std::fmt;

use axum::http::StatusCode;

use crate::agent::per_coord::PerCoordinator;
use crate::cm_agent::turn_budget::TurnBudgetDeny;
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

/// `plan_rewrite_exhausted` 控制面（与 `reason_code` 表一致）；带 `turn_id` / `sub_phase=reflect` / `request_id`。
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
        request_id: None,
    }
    .with_request_id(tracing.and_then(|t| t.request_id.clone()))
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

    /// 与 [`Self::sse_error_payload`] 顶层 `code` 一致；供 `POST /chat` JSON 等 HTTP 层复用。
    pub fn public_error_code(&self) -> &'static str {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => crate::types::SSE_STREAM_CANCELLED_CODE,
                // #1440 K3：配额判定在 `cm_llm::classify_llm_failure`（构造点）完成，这里只读变体。
                LlmCompleteError::RateLimit(_) => "LLM_RATE_LIMIT",
                LlmCompleteError::Transport(_) => "LLM_REQUEST_FAILED",
                LlmCompleteError::Other(_) => "INTERNAL_ERROR",
            },
            Self::TurnAborted { reason, .. } => match reason {
                TurnAbortReason::SseDisconnected => "turn_aborted",
                TurnAbortReason::UserCancelled => crate::types::SSE_STREAM_CANCELLED_CODE,
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

    /// HTTP 建议状态码（与 `ApiError` 同帧返回）；取消类为 **499**（非标准，表示客户端断开/取消）。
    pub fn suggested_http_status(&self) -> StatusCode {
        match self {
            Self::Llm { kind, .. } => match kind {
                LlmCompleteError::Cancelled => {
                    StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
                }
                // 契约：配额 / 限流类**一律**对外 `429`，即使上游是 `402`（余额不足）。
                // 有意丢弃 `e.http_status` 的 402 ——客户端按「退避重试」处理比按 402 语义更安全，
                // 且与改动前行为一致（golden `llm_transport_402` / `llm_transport_quota_msg` 钉住）。
                LlmCompleteError::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
                LlmCompleteError::Transport(e) => e
                    .http_status
                    .and_then(|s| StatusCode::from_u16(s).ok())
                    .unwrap_or(StatusCode::BAD_GATEWAY),
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
            | Self::TurnBudgetExhausted { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    /// `POST /chat` 等 JSON 错误体。
    ///
    /// **HTTP 与 SSE 分流**：SSE 仍可在多种 `code` 下带 **`reason_code`**（见 [`Self::sse_error_payload`]）；
    /// JSON **`ApiError.reason_code`** 仅对 **`INTERNAL_ERROR`** 附带截断内部摘要，避免预算类/取消类路径向客户端泄露编排细节。
    pub(crate) fn http_api_error(&self) -> ApiError {
        if let Self::Llm {
            kind: LlmCompleteError::Transport(_) | LlmCompleteError::RateLimit(_),
            ..
        } = self
        {
            return ApiError::new(self.public_error_code(), self.public_user_message());
        }
        let code = self.public_error_code();
        let message = self.public_user_message();
        if code == "INTERNAL_ERROR" {
            ApiError::with_reason(code, message, self.internal_reason_for_logs())
        } else {
            ApiError::new(code, message)
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

    /// Web `job_id`（与 `x-stream-job-id` / `sse_capabilities` 一致）；CLI 等为 `None`。
    pub fn turn_id(
        tracing: Option<&std::sync::Arc<crate::observability::TracingChatTurn>>,
    ) -> Option<u64> {
        tracing.map(|t| t.job_id)
    }

    pub fn sse_error_payload(&self, turn_id: Option<u64>) -> SseErrorBody {
        self.sse_error_payload_with_request_id(turn_id, None)
    }

    /// 与 [`Self::sse_error_payload`] 相同，并附带可选 **`request_id`**（Web 流任务与 `x-request-id` 对齐）。
    pub fn sse_error_payload_with_request_id(
        &self,
        turn_id: Option<u64>,
        request_id: Option<String>,
    ) -> SseErrorBody {
        let sub_phase = Some(self.sub_phase().as_str().to_string());
        let body = match self {
            Self::Llm { kind, .. } => {
                let (code, user_msg, reason_code) = match kind {
                    LlmCompleteError::Cancelled => (
                        crate::types::SSE_STREAM_CANCELLED_CODE.to_string(),
                        LLM_CANCELLED_ERROR.to_string(),
                        None,
                    ),
                    LlmCompleteError::RateLimit(e) => {
                        ("LLM_RATE_LIMIT".to_string(), e.user_message.clone(), None)
                    }
                    LlmCompleteError::Transport(e) => (
                        "LLM_REQUEST_FAILED".to_string(),
                        e.user_message.clone(),
                        None,
                    ),
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
                    request_id: None,
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
                    request_id: None,
                }
            }
            Self::Other { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("INTERNAL_ERROR".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
                request_id: None,
            },
            Self::StepRetryExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("STEP_RETRY_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
                request_id: None,
            },
            Self::ReplanExhausted { message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some("REPLAN_EXHAUSTED".to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
                request_id: None,
            },
            Self::TurnBudgetExhausted { deny, message, .. } => SseErrorBody {
                error: self.public_user_message(),
                code: Some(budget_deny_error_code(*deny).to_string()),
                reason_code: Some(truncate_reason(message.clone())),
                turn_id,
                sub_phase,
                request_id: None,
            },
        };
        body.with_request_id(request_id)
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
const fn budget_deny_error_code(deny: TurnBudgetDeny) -> &'static str {
    match deny {
        TurnBudgetDeny::WallClock => "TIME_LIMIT_EXHAUSTED",
        TurnBudgetDeny::LlmCalls => "LLM_CALL_LIMIT_EXHAUSTED",
        TurnBudgetDeny::Tokens => "TOKEN_LIMIT_EXHAUSTED",
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
        assert!(
            api.reason_code.is_none(),
            "HTTP JSON omits reason_code for non-INTERNAL_ERROR"
        );
        let sse = e.sse_error_payload(None);
        assert!(
            sse.reason_code
                .as_deref()
                .is_some_and(|s| s.contains("step"))
        );
    }

    #[test]
    fn http_internal_error_includes_reason_other_llm() {
        let e = RunAgentTurnError::Llm {
            phase: AgentTurnSubPhase::Planner,
            kind: LlmCompleteError::Other(Box::new(std::io::Error::other("parse blew up"))),
        };
        let api = e.http_api_error();
        assert_eq!(api.code, "INTERNAL_ERROR");
        assert!(
            api.reason_code
                .as_deref()
                .is_some_and(|s| s.contains("parse"))
        );
    }

    #[test]
    fn http_llm_transport_omits_reason() {
        let e = RunAgentTurnError::Llm {
            phase: AgentTurnSubPhase::Executor,
            kind: LlmCompleteError::from_boxed(Box::new(crate::llm::LlmCallError::from_http_api(
                503,
                "upstream".into(),
            ))),
        };
        let api = e.http_api_error();
        assert_eq!(api.code, "LLM_REQUEST_FAILED");
        assert!(api.reason_code.is_none());
    }

    /// #1440 K3：上游 `503` 的**真实**文案恒含「（HTTP 503）」，历史子串反查会把它误判成
    /// 限流（对外 `429` / `LLM_RATE_LIMIT`）。结构化判定后必须回落成 `LLM_REQUEST_FAILED`，
    /// 并保留 `503`。
    #[test]
    fn http_llm_transport_503_real_shape_is_not_rate_limit() {
        let e = RunAgentTurnError::Llm {
            phase: AgentTurnSubPhase::Executor,
            kind: LlmCompleteError::from_boxed(Box::new(crate::llm::LlmCallError::from_http_api(
                503,
                "模型接口返回错误（HTTP 503），请检查 API 密钥与配额，或稍后重试".into(),
            ))),
        };
        assert_eq!(e.public_error_code(), "LLM_REQUEST_FAILED");
        assert_eq!(e.suggested_http_status().as_u16(), 503);
    }

    #[test]
    /// 反向守卫（#1478）：`Display` 文案**不参与**分流判定。
    ///
    /// 改动前这里断言「`Other { message: 取消串 }` 被当作取消」——那正是文案启发式；生产代码
    /// 从不这样构造，删掉兜底后该错误正确落回 `FailureEmitSseError`，而结构化取消路径照常命中。
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

    #[test]
    fn plan_rewrite_exhausted_carries_tracing_request_id() {
        let tracing =
            crate::observability::TracingChatTurn::new(42, "conv-1", Some("cm-req-xyz".into()));
        let body = sse_plan_rewrite_exhausted_body(
            Some(&tracing),
            "plan_validate_only_node_binding_mismatch",
        );
        assert_eq!(body.code.as_deref(), Some("plan_rewrite_exhausted"));
        assert_eq!(body.turn_id, Some(42));
        assert_eq!(body.request_id.as_deref(), Some("cm-req-xyz"));
        assert_eq!(body.sub_phase.as_deref(), Some("reflect"));
    }

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
