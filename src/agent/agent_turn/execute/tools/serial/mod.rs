use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;

use super::{ExecuteToolsBatchOutcome, ExecuteToolsCommonCtx};
use crate::sse::SseEncoder;

mod after_dispatch;
mod emit;
mod exec_serial;

/// 串行路径：`dispatch_tool`、只读结果缓存、写操作后清缓存（实现体见 [`exec_serial`]）。
pub(super) async fn execute_tools_serial(
    ctx: ExecuteToolsCommonCtx<'_>,
    workspace_changed: &mut bool,
) -> ExecuteToolsBatchOutcome {
    exec_serial::execute_tools_serial_impl(ctx, workspace_changed).await
}

/// 串行工具路径：统一构造 `ToolEnvelopeContext` 并下发 SSE / 追加消息。
pub(super) struct SerialEmitToolResultParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) result: String,
    pub(super) reflection_inject: Option<serde_json::Value>,
    pub(super) encoder: &'a dyn SseEncoder,
}

pub(super) struct SerialTtlRunCommandEarlyHitParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) effective_working_dir: &'a Path,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    pub(super) encoder: &'a dyn SseEncoder,
}

pub(super) struct SerialEarlyToolPolicyDenyParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) step_executor_constraint: Option<PlanStepExecutorKind>,
    pub(super) tools_defs_full: &'a [crate::types::Tool],
    pub(super) turn_allow: Option<&'a HashSet<String>>,
    pub(super) encoder: &'a dyn SseEncoder,
}

pub(super) struct SerialTtlAfterDispatchParams<'a> {
    pub(super) cfg: &'a crate::config::AgentConfig,
    pub(super) effective_working_dir: &'a Path,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) result: &'a str,
    pub(super) workspace_changed: bool,
}

/// 预检 / 策略拒绝 / `run_command` 短路 / 只读缓存命中：若已下发工具结果则返回 `true`（外层应 `continue`）。
pub(super) struct SerialEmitEarlyWithoutDispatchParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) effective_working_dir: &'a Path,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) step_executor_constraint: Option<PlanStepExecutorKind>,
    pub(super) tools_defs_full: &'a [crate::types::Tool],
    pub(super) turn_allow: Option<&'a HashSet<String>>,
    pub(super) readonly_cache: &'a mut HashMap<(String, String), String>,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    pub(super) encoder: &'a dyn SseEncoder,
}

/// 每轮工具迭代开头：记录 tracing、下发 `tool_call` / `timeline`、打调用日志（从 [`execute_tools_serial`] 拆出以降低 nloc）。
pub(super) struct SerialToolIterationSsePreface<'a> {
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_mirror: Option<&'a crate::sse::SseControlMirror>,
    pub(super) cfg: &'a std::sync::Arc<crate::config::AgentConfig>,
    pub(super) tracing_chat_turn: Option<&'a std::sync::Arc<crate::observability::TracingChatTurn>>,
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) messages: &'a [crate::types::Message],
    pub(super) encoder: &'a dyn SseEncoder,
}
