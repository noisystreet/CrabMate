//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web/CLI）与 Axum handler 共用。
//!
//! **目录分组（T4）**：
//! - [`turn_loop`]（目录 `loop/`）：外循环 IO、分发、完成纠偏包装
//! - [`plan_reflect`]：P / R / 回合起点 Act 启发式
//! - [`host`]：工具执行宿主、`RunLoopParams`、TurnSink、错误类型
//!
//! 外循环 FSM / 完成判定核等纯逻辑已下沉 **`crabmate-agent::agent_turn`**；本目录再导出。
//!
//! **与 `llm` 的边界**：本目录内对模型的调用须经 **`llm::complete_chat_retrying`**（见 **`docs/开发文档.md`**「`agent_turn` 与 `llm`：唯一入口与禁止事项」）；**禁止**直接调用 **`llm::api::stream_chat`**。
//!
//! **编排接线**：回合模式分发见 **`run_dispatch`**；ReAct driver 见 **`react_turn`**；主文件保留入口日志、分隔线、`PerCoordinator` 构造与分支调用。

use log::debug;
use tracing::info;

use crate::agent::per_coord::{PerCoordinator, PerCoordinatorInit};

use self::orchestration_entry::{TurnOrchestrationTransition, log_orchestration_transition};

pub(crate) mod host;
pub(crate) mod plan_reflect;
#[path = "loop/mod.rs"]
pub(crate) mod turn_loop;

// ---- 稳定模块路径（对外 / `$crate::agent::agent_turn::…`）----
pub(crate) use host::{
    errors, execute, execute_tools, params, run_command_dedupe, sub_agent_policy, turn_sink,
};
pub(crate) use plan_reflect::{intent, plan, reflect};
#[allow(unused_imports)] // 测试与文档链接：`crate::agent::agent_turn::turn_completion`
pub(crate) use turn_loop::turn_completion;
pub(crate) use turn_loop::{orchestration_entry, run_dispatch};

pub(crate) mod messages {
    pub(crate) use crabmate_agent::agent_turn::messages::*;
}

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
pub(crate) use intent::intent_at_turn_start;
#[allow(unused_imports)]
pub(crate) use messages::push_assistant_merging_trailing_empty_placeholder;
pub(crate) use params::{
    RunLoopAttach, RunLoopCore, RunLoopCtx, RunLoopIo, RunLoopObs, RunLoopParams, RunLoopTurnState,
    TurnPlannerHints,
};
#[allow(unused_imports)]
pub(crate) use plan::{PerPlanCallModelParams, per_plan_call_model_retrying};
#[allow(unused_imports)]
pub(crate) use reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
#[allow(unused_imports)]
pub(crate) use sub_agent_policy::filter_tool_defs_for_executor_kind;
pub(crate) use turn_sink::TurnControlSink;

#[cfg(test)]
mod http_sse_failure_path_golden;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod turn_route_decision_golden;

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    debug!(
        target: "crabmate",
        "run_agent_turn 开始 message_count={} messages_revision={} last_user_preview={} planner_executor_mode={} work_dir={}",
        p.turn.messages().len(),
        p.turn.messages_buffer_revision(),
        crate::redact::last_user_message_preview_for_log(p.turn.messages()),
        p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        p.ctx.core.effective_working_dir.display()
    );
    p.turn.insert_separator_after_last_user_for_turn();

    log_orchestration_transition(TurnOrchestrationTransition::EnterCommon, None, &[]);
    info!(
        target: "crabmate::agent_turn",
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        session_mode = %p.ctx.attach.session_mode,
        "run_agent_turn_common enter"
    );

    let mut per_coord = PerCoordinator::new(PerCoordinatorInit::from_agent_config(
        p.ctx.core.cfg.as_ref(),
    ));

    log_orchestration_transition(TurnOrchestrationTransition::DispatchReAct, None, &[]);
    run_dispatch::dispatch_react_turn(p, &mut per_coord).await
}
