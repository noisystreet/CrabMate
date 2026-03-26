//! R 步：终答阶段（规划校验、是否继续外层循环等）。

use crate::agent::per_coord::{AfterFinalAssistant, PerCoordinator};
use crate::types::Message;

/// R：模型本轮若为最终文本（非 tool_calls），决定是否结束或追加重写提示。
pub(crate) enum ReflectOnAssistantOutcome {
    /// 结束 `run_agent_turn` 外层循环
    StopTurn,
    /// 已写入重写 user 消息，应继续外层循环再次请求模型
    ContinueOuterForPlanRewrite,
    /// 进入工具执行阶段
    ProceedToExecuteTools,
    /// 规划重写次数用尽（已尝试发 SSE 错误码 `plan_rewrite_exhausted`）
    PlanRewriteExhausted,
}

/// 在已将 assistant 推入 `messages` 之后调用，决定是执行工具、终答结束还是规划重写。
///
/// **兼容**：部分 OpenAI 兼容实现在返回 `tool_calls` 时仍上报 `finish_reason: "stop"` 或空串。
/// 若仅判断 `finish_reason == "tool_calls"`，会误判为终答并 `StopTurn`，历史中留下未执行的
/// `tool_calls`、缺对应 `role: tool`，下一轮易 400，且本轮无任何工具执行。故 **非空 `tool_calls`**
/// 同样进入执行分支。
pub(crate) fn per_reflect_after_assistant(
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
    messages: &mut Vec<Message>,
) -> ReflectOnAssistantOutcome {
    if finish_reason == "tool_calls" || msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return ReflectOnAssistantOutcome::ProceedToExecuteTools;
    }
    match per_coord.after_final_assistant(msg, messages.as_slice()) {
        AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        AfterFinalAssistant::RequestPlanRewrite(m) => {
            messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
        AfterFinalAssistant::StopTurnPlanRewriteExhausted => {
            ReflectOnAssistantOutcome::PlanRewriteExhausted
        }
    }
}
