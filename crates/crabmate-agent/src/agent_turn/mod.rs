//! Agent 回合编排中的**纯领域**片段（消息合并、意图后路由、非分层主路径解析）。

pub mod hierarchical_intent_route;
pub mod intent;
pub mod messages;
pub mod staged_planning_gate_types;
pub mod turn_orchestration;

pub use hierarchical_intent_route::{
    HierarchicalDiscourseFallbackReason, HierarchicalPostIntentRoute,
    resolve_hierarchical_post_intent_route,
};
pub use staged_planning_gate_types::{StagedPlanningDenyReason, StagedPlanningGateOutcome};
pub use turn_orchestration::{
    NonHierarchicalEntryResolution, NonHierarchicalMainRoute, NonHierarchicalStagedKind,
    SingleAgentOuterLoopBecause, TurnOrchestrationMode, resolve_non_hierarchical_main_route,
};
