//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web/CLI）与 Axum handler 共用。
//!
//! 子模块：`intent`、`plan`（P）、`reflect`（R）、`execute`（E，实现见 **`execute/tools`**）、`messages`、`staged_sse`（实现见 **`staged/sse`**）、`params`、**`outer_loop_fsm`** + **`outer_loop_reflect`** + **`outer_loop`**、**`staged`**（滚动视界、**`turn_orchestrator_fsm`** / **`steps_loop_route_fsm`** 顶层与步后路由；见 `docs/design/per_state_machine_consolidation.md`）。
//!
//! **与 `llm` 的边界**：本目录内对模型的调用须经 **`llm::complete_chat_retrying`**（见 **`docs/开发文档.md`**「`agent_turn` 与 `llm`：唯一入口与禁止事项」）；**禁止**直接调用 **`llm::api::stream_chat`**。
//!
//! **编排接线**：回合模式分发（分层 / 逻辑双代理 / 分阶段 / 单 Agent）见 **`run_dispatch`**；顶层形态枚举、**`turn_orchestration::NonHierarchicalEntryResolution`**（门控+配置→主路径与单 Agent 外循环根因）与解析见 **`turn_orchestration`**；分层意图后路由纯函数见 **`hierarchical_intent_route`**；主文件保留入口日志、分隔线、`PerCoordinator` 构造与分支调用。

use log::debug;
use tracing::info;

use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};

use self::orchestration_entry::{
    TurnOrchestrationTransition, TurnTopLevelDispatch, log_orchestration_transition,
    resolve_turn_top_level_dispatch,
};

mod completion_suppression;
mod errors;
mod execute;
pub(crate) use execute::tools as execute_tools;
mod hierarchical_intent_route;
mod hierarchy;
mod intent;
mod messages;
mod orchestration_entry;
mod outer_loop;
mod outer_loop_build_idle;
mod outer_loop_fsm;
mod outer_loop_reflect;
mod params;
mod plan;
mod reflect;
mod run_dispatch;
mod staged;
mod sub_agent_policy;
mod task_level_evidence;
mod turn_orchestration;

// 供 crate 内其它模块与文档链接；本文件自身不直接使用这些符号。
pub(crate) use errors::{AgentTurnJobOutcomeKind, AgentTurnSubPhase, RunAgentTurnError};
#[allow(unused_imports)]
pub(crate) use execute::tool_execution_host::{
    CrabmateParallelToolDispatch, CrabmateRegistryToolDispatch, ParallelHttpFetchParams,
};
#[allow(unused_imports)]
pub(crate) use execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
pub(crate) use intent::{intent_at_turn_start, intent_user};
#[allow(unused_imports)]
pub(crate) use messages::push_assistant_merging_trailing_empty_placeholder;
pub(crate) use params::{
    RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams, RunLoopTurnState,
    TurnPlannerHints,
};
#[allow(unused_imports)]
pub(crate) use plan::{
    AgentLlmCall, PerPlanCallModelParams, PlannerSseGate, per_plan_call_model_retrying,
};
#[allow(unused_imports)]
pub(crate) use reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
pub(crate) use sub_agent_policy::filter_tool_defs_for_executor_kind;

#[cfg(test)]
mod tests;

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    if let Some(ctx) = p.ctx.attach.cli_tool_ctx {
        ctx.reset_command_stats();
    }
    debug!(
        target: "crabmate",
        "run_agent_turn 开始 message_count={} messages_revision={} last_user_preview={} staged_plan={} planner_executor_mode={} work_dir={}",
        p.turn.messages().len(),
        p.turn.messages_buffer_revision(),
        crate::redact::last_user_message_preview_for_log(p.turn.messages()),
        p.ctx.core.cfg.staged_planning.staged_plan_execution,
        p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        p.ctx.core.effective_working_dir.display()
    );
    p.turn.insert_separator_after_last_user_for_turn();

    let top_dispatch = resolve_turn_top_level_dispatch(p.ctx.core.cfg.as_ref());
    log_orchestration_transition(
        TurnOrchestrationTransition::EnterCommon,
        None,
        &[("top_level_dispatch", top_dispatch.as_str())],
    );
    info!(
        target: "crabmate::agent_turn",
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        staged_plan_execution = p.ctx.core.cfg.staged_planning.staged_plan_execution,
        intent_at_turn_start_enabled = p.ctx.core.cfg.intent_routing.intent_at_turn_start_enabled,
        top_level_dispatch = top_dispatch.as_str(),
        "run_agent_turn_common enter"
    );

    let mut per_coord = PerCoordinator::new(PerCoordinatorInit::from_agent_config(
        p.ctx.core.cfg.as_ref(),
    ));

    match top_dispatch {
        TurnTopLevelDispatch::Hierarchical => {
            log_orchestration_transition(
                TurnOrchestrationTransition::DispatchHierarchical,
                Some(crate::agent::agent_turn::turn_orchestration::TurnOrchestrationMode::Hierarchical.as_str()),
                &[],
            );
            run_dispatch::dispatch_hierarchical_turn(p, &mut per_coord).await
        }
        TurnTopLevelDispatch::NonHierarchical => {
            log_orchestration_transition(
                TurnOrchestrationTransition::DispatchNonHierarchical,
                None,
                &[],
            );
            run_dispatch::dispatch_non_hierarchical_turn(p, &mut per_coord).await
        }
    }
}
