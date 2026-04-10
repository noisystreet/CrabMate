//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web/CLI）与 Axum handler 共用。
//!
//! 子模块：`messages`（助手合并/分隔线）、`staged_sse`（分阶段 SSE）、`params`（`RunLoopParams`）、`plan_call`（P）、`reflect`（R）、
//! `execute_tools`（E）、`outer_loop`（默认主循环）、`staged`（分阶段与逻辑双 agent）。

use log::debug;

use crate::agent::per_coord::PerCoordinator;
use crate::config::PlannerExecutorMode;

mod execute_tools;
mod messages;
mod outer_loop;
mod params;
mod plan_call;
mod planner_sse_gate;
mod reflect;
mod staged;
mod staged_sse;
mod sub_agent_policy;

// 供 crate 内其它模块与文档链接；本文件自身不直接使用这些符号。
#[allow(unused_imports)]
pub(crate) use execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
#[allow(unused_imports)]
pub(crate) use messages::push_assistant_merging_trailing_empty_placeholder;
pub(crate) use params::RunLoopParams;
#[allow(unused_imports)]
pub(crate) use plan_call::{PerPlanCallModelParams, per_plan_call_model_retrying};
#[allow(unused_imports)]
pub(crate) use reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};

use messages::insert_separator_after_last_user_for_turn;
use outer_loop::run_agent_outer_loop;
use staged::{run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps};

#[cfg(test)]
mod tests;

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    let mut per_coord = PerCoordinator::new(crate::agent::per_coord::PerCoordinatorInit {
        reflection_default_max_rounds: p.cfg.reflection_default_max_rounds,
        final_plan_policy: p.cfg.final_plan_requirement,
        plan_rewrite_max_attempts: p.cfg.plan_rewrite_max_attempts,
        final_plan_require_strict_workflow_node_coverage: p
            .cfg
            .final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled: p.cfg.final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools: p
            .cfg
            .final_plan_semantic_check_max_non_readonly_tools,
    });

    if p.cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent {
        run_logical_dual_agent_then_execute_steps(p, &mut per_coord).await
    } else if p.cfg.staged_plan_execution {
        run_staged_plan_then_execute_steps(p, &mut per_coord).await
    } else {
        run_agent_outer_loop(p, &mut per_coord).await
    }
}
