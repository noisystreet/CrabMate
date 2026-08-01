//! 编排相位词汇对照表（真源）。
//!
//! **三机并存、勿合并为一张全局 FSM**；亦**勿**与前端 `TurnLifecycle` / `StreamControlPhase` 混用。
//!
//! | tracing / SSE 字段 | 权威类型 | 取值（现行） | 职责 |
//! |---|---|---|---|
//! | `turn_orchestration_mode` | [`TurnOrchestrationMode`](super::TurnOrchestrationMode) | `react` · `intent_at_turn_start_finished` | 整轮主执行面 |
//! | `turn_route_*` / `orchestration_route` | [`TurnRouteDecisionV1`](super::TurnRouteDecisionV1) | 见金样 `turn_route_decision_golden` | 门控后一次性路由快照 |
//! | `outer_loop_step` | [`OuterLoopIterationPhase`](super::OuterLoopIterationPhase) | `iteration_enter` … `tools_execute` | 单次外循环迭代粗相位（经 [`OuterLoopDriver`](super::OuterLoopDriver)） |
//! | `sub_phase` | 根包 `AgentTurnSubPhase` | `planner` · `executor` · `reflect` | 错误/SSE **观测**标注（非转移表） |
//! | `gate_phase` / `gate_decision_reason` | `FinalPlanGatePhase` 等 | 见 `per_coord/final_plan_gate` | 终答 `agent_reply_plan` 门控 |
//! | （workflow）`reflection_fsm_phase` | `WorkflowReflectionFsmPhase` | 见 workflow 反思控制器 | 仅 `workflow_execute` 路径 |
//!
//! **唯一非早退出口**：意图门控跑完后，必须经 [`assess_turn_routing`](super::assess_turn_routing)
//! 得到 [`TurnRouteDriver`](super::TurnRouteDriver)；IO 侧（`run_dispatch`）只消费该 driver。
//!
//! 前端 UI 相位（`TurnPhase` / `StreamControlPhase`）属于流式壳层，**禁止**并进本表。

/// 顶层编排模式的稳定字符串（与 [`TurnOrchestrationMode::as_str`](super::TurnOrchestrationMode::as_str) 对齐）。
pub const TURN_ORCHESTRATION_MODE_REACT: &str = "react";
/// 意图门控已写入终答并结束本回合。
pub const TURN_ORCHESTRATION_MODE_INTENT_FINISHED: &str = "intent_at_turn_start_finished";

/// 外循环 `outer_loop_step` 稳定字符串（与 [`OuterLoopIterationPhase::as_str`](super::OuterLoopIterationPhase::as_str) 对齐）。
pub const OUTER_LOOP_STEP_ITERATION_ENTER: &str = "iteration_enter";
/// 上下文准备完成。
pub const OUTER_LOOP_STEP_PREPARE_CONTEXT_DONE: &str = "prepare_context_done";
/// 规划模型调用完成。
pub const OUTER_LOOP_STEP_AFTER_PLANNER_MODEL: &str = "after_planner_model";
/// 反思分支已决。
pub const OUTER_LOOP_STEP_REFLECT_DECIDED: &str = "reflect_decided";
/// 工具批执行中。
pub const OUTER_LOOP_STEP_TOOLS_EXECUTE: &str = "tools_execute";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_turn::{OuterLoopIterationPhase, TurnOrchestrationMode};

    #[test]
    fn turn_orchestration_mode_strings_match_enum() {
        assert_eq!(
            TurnOrchestrationMode::ReAct.as_str(),
            TURN_ORCHESTRATION_MODE_REACT
        );
        assert_eq!(
            TurnOrchestrationMode::IntentAtTurnStartFinished.as_str(),
            TURN_ORCHESTRATION_MODE_INTENT_FINISHED
        );
    }

    #[test]
    fn outer_loop_step_strings_match_enum() {
        assert_eq!(
            OuterLoopIterationPhase::IterationEnter.as_str(),
            OUTER_LOOP_STEP_ITERATION_ENTER
        );
        assert_eq!(
            OuterLoopIterationPhase::PrepareContextDone.as_str(),
            OUTER_LOOP_STEP_PREPARE_CONTEXT_DONE
        );
        assert_eq!(
            OuterLoopIterationPhase::AfterPlannerModel.as_str(),
            OUTER_LOOP_STEP_AFTER_PLANNER_MODEL
        );
        assert_eq!(
            OuterLoopIterationPhase::ReflectDecided.as_str(),
            OUTER_LOOP_STEP_REFLECT_DECIDED
        );
        assert_eq!(
            OuterLoopIterationPhase::ToolsExecute.as_str(),
            OUTER_LOOP_STEP_TOOLS_EXECUTE
        );
    }
}
