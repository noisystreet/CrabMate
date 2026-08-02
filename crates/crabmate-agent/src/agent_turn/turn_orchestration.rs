//! 单轮 **`run_agent_turn`** 顶层编排形态（**非**全局 FSM）：供结构化 `tracing` 与排障对齐 `run_dispatch` 分支。
//!
//! 非分层路径由统一 driver（**`non_hierarchical_turn`**）消费 [`NonHierarchicalTurnPhase`]：**`ReAct`**（仅外循环）。

use crabmate_config::AgentConfig;

/// 非分层回合阶段：外循环（ReAct）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonHierarchicalTurnPhase {
    /// 整轮 `run_agent_outer_loop`（ReAct 循环）。
    ReAct,
}

impl NonHierarchicalTurnPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
        }
    }
}

/// 非分层、启发式已跑完时：聚合配置与 [`NonHierarchicalTurnPhase`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonHierarchicalTurnResolution {
    pub turn_phase: NonHierarchicalTurnPhase,
    pub orchestration_mode: TurnOrchestrationMode,
    /// 仅当 [`NonHierarchicalTurnPhase::ReAct`] 时有值。
    pub freeform_because: Option<ReActBecause>,
}

/// 非分层下走 **`ReAct`** 的根因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReActBecause {
    Freeform,
}

impl ReActBecause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Freeform => "freeform",
        }
    }
}

impl NonHierarchicalTurnResolution {
    /// 非分层直接返回 ReAct（分阶段规划已移除）。
    pub fn resolve_react(cfg: &AgentConfig) -> Self {
        let _ = cfg;
        Self {
            turn_phase: NonHierarchicalTurnPhase::ReAct,
            orchestration_mode: TurnOrchestrationMode::ReAct,
            freeform_because: Some(ReActBecause::Freeform),
        }
    }
}

impl From<NonHierarchicalTurnPhase> for TurnOrchestrationMode {
    fn from(phase: NonHierarchicalTurnPhase) -> Self {
        match phase {
            NonHierarchicalTurnPhase::ReAct => Self::ReAct,
        }
    }
}

/// 本轮实际进入的主执行形态（在已知分支条件后记录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOrchestrationMode {
    /// 整轮 `run_agent_outer_loop`（ReAct 循环）。
    ReAct,
}

impl TurnOrchestrationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
        }
    }
}
