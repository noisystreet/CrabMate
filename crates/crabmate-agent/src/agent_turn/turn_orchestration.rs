//! 单轮 **`run_agent_turn`** 顶层编排形态（**非**全局 FSM）：供结构化 `tracing` 与排障对齐 `run_dispatch` 分支。
//!
//! 真源为 [`TurnOrchestrationMode`]（现行仅 **`ReAct`** 外循环）。勿与前端 UI 的 `TurnPhase` 混用。

use crabmate_config::AgentConfig;

/// 本轮实际进入的主执行形态。
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

/// 走 **`ReAct`** 外循环时的根因标注（供决议 JSON / tracing）。
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

/// 启发式已跑完时的路由决议核。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnResolution {
    pub orchestration_mode: TurnOrchestrationMode,
    pub freeform_because: Option<ReActBecause>,
}

impl TurnResolution {
    /// 恒进 ReAct（分阶段 / 分层路径已移除）。
    pub fn resolve_react(cfg: &AgentConfig) -> Self {
        let _ = cfg;
        Self {
            orchestration_mode: TurnOrchestrationMode::ReAct,
            freeform_because: Some(ReActBecause::Freeform),
        }
    }
}
