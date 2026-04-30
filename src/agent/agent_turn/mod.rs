//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web/CLI）与 Axum handler 共用。
//!
//! 子模块：`intent`、`plan`（P）、`reflect`（R）、`execute`（E，实现见 **`execute/tools`**）、`messages`、`staged_sse`（实现见 **`staged/sse`**）、`params`、`outer_loop`、`staged`（含 **`staged/orchestrator`**、**`staged/patch_planner`**）。
//!
//! **与 `llm` 的边界**：本目录内对模型的调用须经 **`llm::complete_chat_retrying`**（见 **`docs/DEVELOPMENT.md`**「`agent_turn` 与 `llm`：唯一入口与禁止事项」）；**禁止**直接调用 **`llm::api::stream_chat`**。

use log::debug;

use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};
use crate::agent::{
    intent_l0,
    intent_pipeline::{IntentAction, IntentContext, assess_and_route},
    intent_router::ExecuteIntentThresholds,
};
use crate::config::PlannerExecutorMode;
use crate::types::Message;

mod errors;
mod execute;
pub(crate) use execute::tools as execute_tools;
mod hierarchy;
mod intent;
mod messages;
mod outer_loop;
mod params;
mod plan;
mod reflect;
mod staged;
mod sub_agent_policy;

// 供 crate 内其它模块与文档链接；本文件自身不直接使用这些符号。
pub(crate) use errors::{AgentTurnSubPhase, RunAgentTurnError};
#[allow(unused_imports)]
pub(crate) use execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
pub(crate) use intent::{intent_at_turn_start, intent_user};
#[allow(unused_imports)]
pub(crate) use messages::push_assistant_merging_trailing_empty_placeholder;
pub(crate) use params::RunLoopParams;
#[allow(unused_imports)]
pub(crate) use plan::{
    AgentLlmCall, PerPlanCallModelParams, PlannerSseGate, per_plan_call_model_retrying,
};
#[allow(unused_imports)]
pub(crate) use reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
pub(crate) use sub_agent_policy::filter_tool_defs_for_executor_kind;

use messages::insert_separator_after_last_user_for_turn;
use outer_loop::run_agent_outer_loop;
use staged::{run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps};

#[cfg(test)]
mod tests;

const STAGED_INTENT_GATE_RECENT_USER_FOR_MERGE: usize = 4;
const STAGED_INTENT_GATE_MSG_TAIL_FOR_TOOL: usize = 32;

fn should_enter_staged_planning(messages: &[Message], cfg: &crate::config::AgentConfig) -> bool {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(messages);
    let task = intent_user::extract_effective_user_task(messages, in_clarification_flow);
    if task.trim().is_empty() {
        return false;
    }
    let has_recent_tool_failure = intent_l0::messages_have_recent_tool_failure(
        messages,
        STAGED_INTENT_GATE_MSG_TAIL_FOR_TOOL,
    );
    let recent_user_messages = intent_user::collect_recent_user_messages(
        messages,
        STAGED_INTENT_GATE_RECENT_USER_FOR_MERGE,
    );
    let intent_ctx = IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds: ExecuteIntentThresholds {
            low: cfg.intent_non_hier_execute_low_threshold,
            high: cfg.intent_non_hier_execute_high_threshold,
        },
        l2_min_confidence: cfg.intent_l2_min_confidence,
        has_recent_tool_failure,
        l0_routing_boost_enabled: cfg.intent_l0_routing_boost_enabled,
    };
    let decision = assess_and_route(task.as_str(), &intent_ctx);
    let allowed = matches!(decision.action, IntentAction::Execute);
    log::info!(
        target: "crabmate",
        "staged_plan_intent_gate task_preview={} kind={:?} primary={} action={:?} allowed={}",
        crate::redact::preview_chars(task.as_str(), 80),
        decision.kind,
        decision.primary_intent,
        decision.action,
        allowed
    );
    allowed
}

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    if let Some(ctx) = p.cli_tool_ctx {
        ctx.reset_command_stats();
    }
    debug!(
        target: "crabmate",
        "run_agent_turn 开始 message_count={} last_user_preview={} staged_plan={} planner_executor_mode={} work_dir={}",
        p.messages.len(),
        crate::redact::last_user_message_preview_for_log(p.messages),
        p.cfg.staged_plan_execution,
        p.cfg.planner_executor_mode.as_str(),
        p.effective_working_dir.display()
    );
    insert_separator_after_last_user_for_turn(p.messages);

    let mut per_coord = PerCoordinator::new(PerCoordinatorInit::from_agent_config(p.cfg.as_ref()));

    if p.cfg.planner_executor_mode == PlannerExecutorMode::Hierarchical {
        // 意图门控在 `hierarchy::run_hierarchical_agent` 内通过 `run_intent_for_hierarchical` 执行（与 L0/合并文本一致），勿在此重复。
        log::info!(target: "crabmate", "run_agent_turn: using Hierarchical mode");
        hierarchy::run_hierarchical_agent(p).await
    } else {
        if !intent_at_turn_start::run_intent_at_turn_start_if_configured(p).await? {
            return Ok(());
        }
        let allow_staged = should_enter_staged_planning(p.messages, p.cfg.as_ref());
        if p.cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent && allow_staged {
            log::info!(target: "crabmate", "run_agent_turn: using LogicalDualAgent mode");
            run_logical_dual_agent_then_execute_steps(p, &mut per_coord).await
        } else if p.cfg.staged_plan_execution && allow_staged {
            log::info!(target: "crabmate", "run_agent_turn: using staged_plan mode");
            run_staged_plan_then_execute_steps(p, &mut per_coord).await
        } else {
            log::info!(target: "crabmate", "run_agent_turn: using single_agent mode");
            run_agent_outer_loop(p, &mut per_coord).await
        }
    }
}
