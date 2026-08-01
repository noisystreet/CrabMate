//! Agent 回合编排中的**纯领域**片段（消息合并、意图后路由、非分层主路径解析、外循环 FSM、完成判定）。
//!
//! **相位词汇真源**：见 [`phase_vocabulary`]（`turn_orchestration_mode` / `outer_loop_step` / `sub_phase` / Gate 对照）。
//! **路由入口**：意图门控之后唯一决议函数为 [`assess_turn_routing`]；根包 `run_dispatch` 只消费 [`TurnRouteDriver`]。

pub mod completion_suppression;
pub mod intent;
pub mod intent_routing;
pub mod messages;
pub mod orchestration_entry;
pub mod outer_loop_driver;
pub mod outer_loop_fsm;
pub mod outer_loop_iteration_reduce;
#[cfg(test)]
mod outer_loop_phase_golden;
pub mod outer_loop_reflect_reason;
pub mod phase_vocabulary;
pub mod run_command_dedupe;
pub mod task_level_evidence;
pub mod tool_execution;
pub mod turn_completion_decision;
pub mod turn_orchestration;
pub mod turn_route_decision;

pub use completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    redundant_tool_names_for_log, tool_call_is_redundant_after_completion,
    tool_calls_are_redundant_after_completion, tool_calls_are_redundant_when_goal_satisfied,
};
pub use intent_routing::{
    IntentL2ClassifierHost, IntentRoutingOutcome, IntentRoutingPipelineParams,
    assess_intent_routing_full_pipeline, assess_intent_routing_with_optional_l2,
    log_intent_pipeline_assessment,
};
pub use orchestration_entry::{
    TurnOrchestrationTransition, TurnTopLevelDispatch, log_orchestration_transition,
    resolve_turn_top_level_dispatch,
};
pub use outer_loop_driver::OuterLoopDriver;
pub use outer_loop_fsm::{OuterLoopIterationExit, OuterLoopIterationPhase, ReflectBranchCtl};
pub use outer_loop_iteration_reduce::{
    OuterLoopReflectReduceAction, outer_loop_iteration_exit_from_reflect_reduce,
    reduce_outer_loop_post_tools_exit, reduce_outer_loop_reflect_branch,
};
pub use outer_loop_reflect_reason::OuterLoopReflectPreGateReason;
pub use phase_vocabulary::{
    OUTER_LOOP_STEP_AFTER_PLANNER_MODEL, OUTER_LOOP_STEP_ITERATION_ENTER,
    OUTER_LOOP_STEP_PREPARE_CONTEXT_DONE, OUTER_LOOP_STEP_REFLECT_DECIDED,
    OUTER_LOOP_STEP_TOOLS_EXECUTE, TURN_ORCHESTRATION_MODE_INTENT_FINISHED,
    TURN_ORCHESTRATION_MODE_REACT,
};
pub use task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};
pub use tool_execution::{
    ExecuteToolsBatchOutcome, ToolBatchExecutionMode, ToolBatchModeParams,
    ToolPolicyEarlyDenyParams, dedup_readonly_tool_calls_count, replay_force_serial_from_env,
    resolve_tool_batch_execution_mode, tool_policy_early_deny_message,
};
pub use turn_completion_decision::{
    TurnCompletionDecision, evaluate_turn_early_stop, evaluate_turn_redundant_tools,
    evaluate_turn_suppress_replanning, log_turn_completion_decision,
};
pub use turn_orchestration::{
    NonHierarchicalTurnPhase, NonHierarchicalTurnResolution, ReActBecause, TurnOrchestrationMode,
};
pub use turn_route_decision::{
    AssessTurnRoutingParams, AssessedTurnRoute, IntentGateSnapshot, TurnRouteDecisionV1,
    TurnRouteDriver, assess_turn_routing, build_non_hierarchical_intent_finished_early_decision,
    build_non_hierarchical_turn_route_decision, intent_action_label, intent_gate_is_early_exit,
    intent_gate_snapshot_finished_early, intent_gate_snapshot_from_decision, intent_kind_label,
    log_turn_route_decision,
};
