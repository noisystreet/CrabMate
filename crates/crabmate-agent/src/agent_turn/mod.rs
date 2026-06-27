//! Agent 回合编排中的**纯领域**片段（消息合并、意图后路由、非分层主路径解析）。

pub mod hierarchical_intent_route;
pub mod intent;
pub mod intent_routing;
pub mod messages;
pub mod staged_planning_gate;
pub mod staged_planning_gate_types;
pub mod tool_execution;
pub mod turn_orchestration;

pub use hierarchical_intent_route::{
    HierarchicalDiscourseFallbackReason, HierarchicalPostIntentRoute,
    resolve_hierarchical_post_intent_route,
};
pub use intent_routing::{
    IntentL2ClassifierHost, IntentRoutingOutcome, IntentRoutingPipelineParams,
    assess_intent_routing_full_pipeline, assess_intent_routing_with_optional_l2,
    log_intent_pipeline_assessment,
};
pub use staged_planning_gate::{
    assess_staged_planning_gate_l1, staged_plan_eligibility_for_intent,
    staged_planning_gate_outcome_from_decision,
};
pub use staged_planning_gate_types::{StagedPlanningDenyReason, StagedPlanningGateOutcome};
pub use tool_execution::{
    ExecuteToolsBatchOutcome, ParallelPrefetchFailureKey, ParallelPrefetchFailures,
    ParallelPrefetchParams, ToolBatchExecutionMode, ToolBatchModeParams, ToolExecutionHost,
    ToolPolicyEarlyDenyParams, dedup_readonly_tool_calls_count, replay_force_serial_from_env,
    resolve_tool_batch_execution_mode, tool_policy_early_deny_message,
};
pub use turn_orchestration::{
    NonHierarchicalEntryResolution, NonHierarchicalMainRoute, NonHierarchicalStagedKind,
    SingleAgentOuterLoopBecause, TurnOrchestrationMode, resolve_non_hierarchical_main_route,
};
