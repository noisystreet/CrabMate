//! 并行/串行工具批共用：信封上下文与结果 SSE 下发。

use crate::agent::per_coord::PerCoordinator;
use crate::types::Message;

use super::emit::emit_tool_result_sse_and_append;
use super::emit_common::{tool_envelope_context, summarize_tool_call_args};
use super::EmitToolResultParams;

pub(super) struct EmitToolBatchResultParams<'a> {
    pub messages: &'a mut Vec<Message>,
    pub per_coord: &'a mut PerCoordinator,
    pub cfg: &'a std::sync::Arc<crate::config::AgentConfig>,
    pub tool_outcome_recorder: &'a std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub control: crate::agent::agent_turn::TurnControlSink<'a>,
    pub tool_result_envelope_v1: bool,
    pub name: &'a str,
    pub args: &'a str,
    pub id: &'a str,
    pub result: String,
    pub reflection_inject: Option<serde_json::Value>,
    pub execution_mode: &'static str,
    pub parallel_batch_id: Option<&'a str>,
}

/// 串行/并行批共用：带执行模式信封的 tool 结果 SSE + messages 追加。
pub(super) async fn emit_tool_batch_result(
    p: EmitToolBatchResultParams<'_>,
    encoder: &dyn crate::sse::SseEncoder,
) {
    let envelope_ctx = tool_envelope_context(p.id, p.execution_mode, p.parallel_batch_id);
    emit_tool_result_sse_and_append(
        p.messages,
        p.per_coord,
        EmitToolResultParams {
            cfg: p.cfg,
            tool_outcome_recorder: p.tool_outcome_recorder,
            control: p.control,
            tool_result_envelope_v1: p.tool_result_envelope_v1,
            name: p.name,
            args: p.args,
            id: p.id,
            result: p.result,
            reflection_inject: p.reflection_inject,
            envelope_ctx: Some(envelope_ctx),
        },
        encoder,
    )
    .await;
}

/// 工具调用摘要（解析 args JSON 后走 `summarize_tool_call_parsed`）。
pub(super) fn tool_call_summary_from_args(name: &str, args: &str) -> String {
    summarize_tool_call_args(name, args).unwrap_or_else(|| format!("tool: {name}"))
}
