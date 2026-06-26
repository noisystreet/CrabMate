//! 回合起点意图门控：从用户消息抽取任务、L0/L1/L2 管线与非分层模式的「开局」门控。
//!
//! 文件拆为 `user`（用户消息侧辅助）、`at_turn_start`（门控主逻辑）与 **`staged_planning_gate`**（非分层分阶段路由门控）。对 [`super`] 仍以
//! `intent_user` / `intent_at_turn_start` 名称 re-export，避免大范围改动调用方。

pub(crate) mod at_turn_start;
pub(crate) mod context;
pub(crate) mod staged_planning_gate;
pub(crate) mod user;

pub(crate) use at_turn_start as intent_at_turn_start;
pub(crate) use context::build_intent_routing_context;
pub(crate) use crabmate_agent::agent_turn::intent::{advisory_bypass, readonly_overview_bypass};
pub(crate) use staged_planning_gate::{
    StagedPlanningGateOutcome, assess_staged_planning_gate_full_pipeline,
};
pub(crate) use user as intent_user;
