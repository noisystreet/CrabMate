//! 单轮 Agent 编排层错误的 **HTTP / SSE 映射**（类型本体在 [`crate::turn_error`]，#1510）。
//!
//! 留在这里的原因很具体：`suggested_http_status` 要 `axum::StatusCode`、`http_api_error` 要
//! `ApiError`、`sse_error_payload` 要 `SseErrorBody`、`turn_id` 要 `observability` —— 这些是
//! 「怎么表达给客户端」，属实现侧。类型与纯判定（含队列分流）已下沉到 [`crate::turn_error`]，
//! 端口层 `turn_runner` 因此不必再反向依赖 `agent`。
//!
//! 下面 `pub use` 保持既有 `crate::agent::agent_turn::host::errors::{…}` 路径不变。

use axum::http::StatusCode;

use crate::agent::per_coord::PerCoordinator;
use crate::llm::LlmCompleteError;
use crate::sse::SseErrorBody;
use crate::turn_error::budget_deny_error_code;
use crate::types::LLM_CANCELLED_ERROR;
use crate::web::http_types::chat::ApiError;

// 名字转发：`errors` 本身是 `pub(crate) mod`，所以这里 `pub(crate) use` 已足够，
// 不必写 `pub use`（那会让人误以为多了一层对外可见性）。#1510 拆出 `turn_error` 后
// 既有 `crate::agent::agent_turn::host::errors::{…}` 调用点靠这条零涟漪。
pub(crate) use crate::turn_error::{
    AgentTurnJobOutcomeKind, AgentTurnSubPhase, RunAgentTurnError, TurnAbortReason,
};

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

impl RunAgentTurnError {
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
}
