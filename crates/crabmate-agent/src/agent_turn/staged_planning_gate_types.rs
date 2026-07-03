//! 非分层分阶段意图门控结果类型（纯数据；异步评估仍在根包 `agent_turn/intent/staged_planning_gate`）。

use crate::intent_pipeline::IntentDecision;
use crate::intent_router::IntentKind;

/// 非分层路径下，是否允许进入分阶段 / 逻辑双代理编排（仅 `IntentAction::Execute` 为 true）。
#[derive(Debug, Clone, PartialEq)]
pub enum StagedPlanningGateOutcome {
    /// 意图管线判定为「执行任务」，可分流到 staged / logical dual。
    Allow {
        task_preview: String,
        intent_kind: IntentKind,
        primary_intent: String,
        confidence: f32,
        decision: IntentDecision,
    },
    /// 无可路由的有效 user 任务句，或管线未给出 Execute，或在开启 advisory bypass 时命中咨询启发式而跳过滚动分阶段规划。
    Deny {
        reason: StagedPlanningDenyReason,
        task_preview: Option<String>,
        intent_decision: Option<IntentDecision>,
    },
}

/// 拒绝进入分阶段编排的原因（用于日志与单测；不含机密）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedPlanningDenyReason {
    /// `extract_effective_user_task` 为空（无 user 或全文空白）。
    EmptyEffectiveTask,
    /// 管线已跑通，但 `action != Execute`（直接回复 / 澄清 / 确认等）。
    IntentPipelineNotExecute,
    /// 管线判定为 **Execute**，且 advisory bypass 命中「架构/重构咨询」启发式。
    AdvisoryExecuteBypassStaged,
    /// 只读概览/探查类 Execute：不进入分阶段规划。
    ReadonlyOverviewBypassStaged,
    /// 编排档位 `freeform` 强制外循环。
    OrchestrationProfileFreeform,
}

impl StagedPlanningDenyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyEffectiveTask => "empty_effective_task",
            Self::IntentPipelineNotExecute => "intent_pipeline_not_execute",
            Self::AdvisoryExecuteBypassStaged => "advisory_execute_bypass_staged",
            Self::ReadonlyOverviewBypassStaged => "readonly_overview_bypass_staged",
            Self::OrchestrationProfileFreeform => "orchestration_profile_freeform",
        }
    }
}

impl StagedPlanningGateOutcome {
    pub fn allows_staged_planning(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}
