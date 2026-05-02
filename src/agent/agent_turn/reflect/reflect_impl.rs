//! R 步：终答阶段（规划校验、是否继续外层循环等）。

use crate::agent::per_coord::final_plan_gate;
use crate::agent::per_coord::{AfterFinalAssistant, PerCoordinator, PlanRewriteExhaustedReason};
use crate::agent::per_plan_semantic_check::{self, PlanSemanticLlmCtx};
use crate::types::Message;

use super::super::params::RunLoopParams;

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
    p.turn.sub_phase = crate::agent::agent_turn::AgentTurnSubPhase::Reflect;
    if finish_reason == "tool_calls" || msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return ReflectOnAssistantOutcome::ProceedToExecuteTools;
    }
    match per_coord.after_final_assistant(
        msg,
        p.turn.messages.as_slice(),
        p.ctx.cfg.as_ref(),
        p.ctx.workspace_is_set,
    ) {
        AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        AfterFinalAssistant::RequestPlanRewrite(m) => {
            p.turn.messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
        AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason } => {
            ReflectOnAssistantOutcome::PlanRewriteExhausted { reason }
        }
        AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { plan, tool_digest } => {
            let plan_json = per_plan_semantic_check::agent_reply_plan_json_compact(&plan);
            let outcome = per_plan_semantic_check::evaluate_plan_consistency_with_recent_tools_llm(
                PlanSemanticLlmCtx {
                    llm_backend: p.ctx.llm_backend,
                    client: p.ctx.client,
                    api_key: p.ctx.api_key,
                    cfg: p.ctx.cfg.as_ref(),
                    out: p.ctx.out,
                    no_stream: p.ctx.no_stream,
                    cancel: p.ctx.cancel,
                    plain_terminal_stream: p.ctx.plain_terminal_stream,
                    request_chrome_trace: p.ctx.request_chrome_trace.clone(),
                    temperature_override: p.turn.temperature_override,
                    model_override: p.turn.model_override.clone(),
                    seed_override: p.turn.seed_override,
                    max_tokens: p.ctx.cfg.final_plan_semantic_check_max_tokens,
                },
                plan_json.as_str(),
                tool_digest.as_deref(),
            )
            .await;
            let sem_outcome = final_plan_gate::run_final_plan_gate_semantic_completed(
                &outcome,
                per_coord.plan_rewrite_attempts_snapshot(),
                per_coord.plan_rewrite_max_attempts_limit(),
            );
            tracing::debug!(
                target: "crabmate::agent_turn",
                gate_route = ?sem_outcome.route,
                gate_phase = ?final_plan_gate::FinalPlanGatePhase::PendingSemanticLlm,
                sub_phase = "reflect",
                "final_plan_gate semantic transition"
            );
            match sem_outcome.after {
                AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
                AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason } => {
                    ReflectOnAssistantOutcome::PlanRewriteExhausted { reason }
                }
                AfterFinalAssistant::RequestPlanRewrite(m) => {
                    per_coord.increment_plan_rewrite_attempts();
                    p.turn.messages.push(m);
                    ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
                }
                AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { .. } => unreachable!(
                    "run_final_plan_gate_semantic_completed must not return StopTurnPendingPlanConsistencyLlm"
                ),
            }
        }
    }
}
