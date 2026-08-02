//! 回合顶层编排：结构化阶段迁移标签（与 `tracing` 字段 `orchestration_transition` 对齐）。
//!
//! 现行无多路顶层分发；入口直接进 ReAct。本模块只保留迁移日志标签。

/// 结构化阶段迁移标签（顶层；不含外循环 P/R/E 细粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOrchestrationTransition {
    EnterCommon,
    DispatchReAct,
    ReActEntryResolved,
}

impl TurnOrchestrationTransition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnterCommon => "enter_common",
            Self::DispatchReAct => "dispatch_react",
            Self::ReActEntryResolved => "react_entry_resolved",
        }
    }
}

/// 统一 info 日志字段，减少 `mod.rs` / `run_dispatch` 散落叙述。
pub fn log_orchestration_transition(
    transition: TurnOrchestrationTransition,
    turn_orchestration_mode: Option<&str>,
    extra: &[(&str, &str)],
) {
    let mode = turn_orchestration_mode.unwrap_or("");
    if extra.is_empty() {
        log::info!(
            target: "crabmate::agent_turn",
            "orchestration transition transition={} turn_orchestration_mode={}",
            transition.as_str(),
            mode
        );
    } else {
        let extras: String = extra
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        log::info!(
            target: "crabmate::agent_turn",
            "orchestration transition transition={} turn_orchestration_mode={} {extras}",
            transition.as_str(),
            mode
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_turn::{ReActBecause, TurnOrchestrationMode, TurnResolution};
    use crabmate_config::PlannerExecutorMode;

    fn cfg_single_agent() -> crabmate_config::AgentConfig {
        let mut c = crabmate_config::load_config(None).expect("embed default config");
        c.per_plan_policy.planner_executor_mode = PlannerExecutorMode::SingleAgent;
        c
    }

    #[test]
    fn resolve_react_returns_freeform() {
        let r = TurnResolution::resolve_react(&cfg_single_agent());
        assert_eq!(r.orchestration_mode, TurnOrchestrationMode::ReAct);
        assert_eq!(r.freeform_because, Some(ReActBecause::Freeform));
        assert!(PlannerExecutorMode::parse("hierarchical").is_err());
        assert!(PlannerExecutorMode::parse("logical_dual_agent").is_err());
    }

    #[test]
    fn transition_labels_are_stable() {
        assert_eq!(
            TurnOrchestrationTransition::EnterCommon.as_str(),
            "enter_common"
        );
        assert_eq!(
            TurnOrchestrationTransition::DispatchReAct.as_str(),
            "dispatch_react"
        );
        assert_eq!(
            TurnOrchestrationTransition::ReActEntryResolved.as_str(),
            "react_entry_resolved"
        );
    }
}
