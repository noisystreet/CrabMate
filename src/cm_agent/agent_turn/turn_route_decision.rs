//! 单轮编排路由决议快照（v1 JSON）：Act 句启发式之后一次性记录，供 tracing / SSE / 金样回归。
//!
//! **入口契约**：启发式跑完后，**唯一**决议函数是 [`assess_turn_routing`]；根包 `run_dispatch`
//! 只按 [`TurnRouteDriver`] 进入 ReAct。相位字段含义见 [`super::phase_vocabulary`]。

use crate::cm_config::{AgentConfig, FinalPlanRequirementMode};

use super::turn_orchestration::TurnResolution;

/// 路由决议 JSON 根（`version` 固定为 1）。
///
/// 主执行面只记 [`orchestration_mode`](TurnRouteDecisionV1::orchestration_mode)
///（勿再拆 `top_level` / `turn_phase` 镜像字段）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TurnRouteDecisionV1 {
    pub version: u8,
    pub turn_start: TurnStartSnapshot,
    pub orchestration_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeform_because: Option<String>,
    pub planner_executor_mode: String,
    pub plan_requirement_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_profile: Option<String>,
}

/// 回合起点启发式快照（Ask/Plan 跳过；Act 可挂只读约束）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TurnStartSnapshot {
    /// Ask/Plan：不跑 Act 句启发式（只读由 session_mode 挂载）。
    Disabled,
    EmptyTask,
    /// Act：已跑关键词启发式（`review_readonly` 表示是否收窄工具）。
    ActHeuristics {
        review_readonly: bool,
    },
}

impl TurnRouteDecisionV1 {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

fn plan_requirement_policy_label(cfg: &AgentConfig) -> String {
    match cfg.per_plan_policy.final_plan_requirement {
        FinalPlanRequirementMode::Never => "never".to_string(),
        FinalPlanRequirementMode::WorkflowReflection => "workflow_reflection".to_string(),
        FinalPlanRequirementMode::Always => "always".to_string(),
    }
}

/// 启发式结束后组装决议。
pub fn build_turn_route_decision(
    cfg: &AgentConfig,
    turn_start: TurnStartSnapshot,
    entry: &TurnResolution,
) -> TurnRouteDecisionV1 {
    TurnRouteDecisionV1 {
        version: 1,
        turn_start,
        orchestration_mode: entry.orchestration_mode.as_str().to_string(),
        freeform_because: entry.freeform_because.map(|b| b.as_str().to_string()),
        planner_executor_mode: cfg
            .per_plan_policy
            .planner_executor_mode
            .as_str()
            .to_string(),
        plan_requirement_policy: plan_requirement_policy_label(cfg),
        orchestration_profile: Some(
            cfg.per_plan_policy
                .orchestration_profile
                .as_str()
                .to_string(),
        ),
    }
}

/// 启发式之后的下一执行 driver（纯数据；IO 由 `run_dispatch` 执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRouteDriver {
    /// 外循环（ReAct）。
    ReAct,
}

/// [`assess_turn_routing`] 聚合输出：决议快照 + driver。
#[derive(Debug, Clone, PartialEq)]
pub struct AssessedTurnRoute {
    pub decision: TurnRouteDecisionV1,
    pub driver: TurnRouteDriver,
}

/// 纯函数入参：启发式结束后一次性评估。
#[derive(Debug)]
pub struct AssessTurnRoutingParams<'a> {
    pub cfg: &'a AgentConfig,
    pub turn_start: TurnStartSnapshot,
}

/// 回合起点启发式结束后的**唯一**路由决议：恒进 ReAct 外循环。
///
/// 调用方（`run_dispatch`）不得在本函数之外再分支「是否进外循环」。
pub fn assess_turn_routing(params: AssessTurnRoutingParams<'_>) -> AssessedTurnRoute {
    let entry = TurnResolution::resolve_react(params.cfg);
    let decision = build_turn_route_decision(params.cfg, params.turn_start.clone(), &entry);
    AssessedTurnRoute {
        decision,
        driver: TurnRouteDriver::ReAct,
    }
}

/// 结构化 tracing（与 `log_orchestration_transition` 字段对齐）。
pub fn log_turn_route_decision(decision: &TurnRouteDecisionV1) {
    log::info!(
        target: "crabmate::agent_turn",
        "turn_route_decision version={} orchestration_mode={} freeform_because={} planner_executor_mode={} plan_requirement_policy={}",
        decision.version,
        decision.orchestration_mode,
        decision.freeform_because.as_deref().unwrap_or(""),
        decision.planner_executor_mode,
        decision.plan_requirement_policy,
    );
}
