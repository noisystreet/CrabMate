//! Agent 回合编排中的**纯领域**片段（消息合并、意图后路由、非分层主路径解析）。

pub mod intent;
pub mod intent_routing;
pub mod messages;
pub mod orchestration_entry;
pub mod tool_execution;
pub mod turn_orchestration;
pub mod turn_route_decision;

pub use intent_routing::{
    IntentL2ClassifierHost, IntentRoutingOutcome, IntentRoutingPipelineParams,
    assess_intent_routing_full_pipeline, assess_intent_routing_with_optional_l2,
    log_intent_pipeline_assessment,
};
pub use orchestration_entry::{
    TurnOrchestrationTransition, TurnTopLevelDispatch, log_orchestration_transition,
    resolve_turn_top_level_dispatch,
};
pub use tool_execution::{
    ExecuteToolsBatchOutcome, ToolBatchExecutionMode, ToolBatchModeParams,
    ToolPolicyEarlyDenyParams, dedup_readonly_tool_calls_count, replay_force_serial_from_env,
    resolve_tool_batch_execution_mode, tool_policy_early_deny_message,
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
