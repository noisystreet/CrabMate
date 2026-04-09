//! R 步：终答阶段（规划校验、是否继续外层循环等）。

use crate::agent::per_coord::{AfterFinalAssistant, PerCoordinator, PlanRewriteExhaustedReason};
use crate::agent::per_plan_semantic_check::{self, PlanSemanticLlmCtx};
use crate::types::Message;

use super::params::RunLoopParams;

/// R：模型本轮若为最终文本（非 tool_calls），决定是否结束或追加重写提示。
pub(crate) enum ReflectOnAssistantOutcome {
    /// 结束 `run_agent_turn` 外层循环
    StopTurn,
    /// 已写入重写 user 消息，应继续外层循环再次请求模型
    ContinueOuterForPlanRewrite,
    /// 进入工具执行阶段
    ProceedToExecuteTools,
    /// 规划重写次数用尽（已尝试发 SSE 错误码 `plan_rewrite_exhausted` + `reason_code`）
    PlanRewriteExhausted { reason: PlanRewriteExhaustedReason },
}

/// 在已将 assistant 推入 `messages` 之后调用，决定是执行工具、终答结束还是规划重写。
///
/// **兼容**：部分 OpenAI 兼容实现在返回 `tool_calls` 时仍上报 `finish_reason: "stop"` 或空串。
/// 若仅判断 `finish_reason == "tool_calls"`，会误判为终答并 `StopTurn`，历史中留下未执行的
/// `tool_calls`、缺对应 `role: tool`，下一轮易 400，且本轮无任何工具执行。故 **非空 `tool_calls`**
/// 同样进入执行分支。
pub(crate) async fn per_reflect_after_assistant(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
) -> ReflectOnAssistantOutcome {
    if finish_reason == "tool_calls" || msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return ReflectOnAssistantOutcome::ProceedToExecuteTools;
    }
    match per_coord.after_final_assistant(
        msg,
        p.messages.as_slice(),
        p.cfg.as_ref(),
        p.workspace_is_set,
    ) {
        AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        AfterFinalAssistant::RequestPlanRewrite(m) => {
            p.messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
        AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason } => {
            ReflectOnAssistantOutcome::PlanRewriteExhausted { reason }
        }
        AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { plan, tool_digest } => {
            let plan_json = per_plan_semantic_check::agent_reply_plan_json_compact(&plan);
            let ok = per_plan_semantic_check::plan_consistent_with_recent_tools_llm(
                PlanSemanticLlmCtx {
                    llm_backend: p.llm_backend,
                    client: p.client,
                    api_key: p.api_key,
                    cfg: p.cfg.as_ref(),
                    out: p.out,
                    no_stream: p.no_stream,
                    cancel: p.cancel,
                    plain_terminal_stream: p.plain_terminal_stream,
                    request_chrome_trace: p.request_chrome_trace.clone(),
                    temperature_override: p.temperature_override,
                    seed_override: p.seed_override,
                    max_tokens: p.cfg.final_plan_semantic_check_max_tokens,
                },
                plan_json.as_str(),
                tool_digest.as_deref(),
            )
            .await;
            if ok {
                ReflectOnAssistantOutcome::StopTurn
            } else if per_coord.plan_rewrite_attempts_snapshot()
                >= per_coord.plan_rewrite_max_attempts_limit()
            {
                ReflectOnAssistantOutcome::PlanRewriteExhausted {
                    reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent,
                }
            } else {
                per_coord.increment_plan_rewrite_attempts();
                p.messages
                    .push(PerCoordinator::plan_semantic_mismatch_rewrite_message());
                ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
            }
        }
    }
}
