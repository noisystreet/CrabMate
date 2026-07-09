//! 分阶段 **`staged_plan_intent_gate_advisory_bypass`** 用的「咨询类 Execute → 绕过分阶段」启发式。
//!
//! 内置中英文关键词表可经 **[`crate::config::StagedPlanningConfig`]** 中三个 `*_extra_*` 列表**追加**（运行时一律小写匹配）。

use crate::intent_pipeline::{IntentAction, IntentDecision};
use crabmate_config::StagedPlanningConfig;

const DEFAULT_IMPL_STRENGTH: &[&str] = &[
    "请修改",
    "请实现",
    "请添加",
    "请删除",
    "帮我改",
    "帮我写",
    "帮我删",
    "直接改",
    "直接写",
    "运行 cargo",
    "cargo test",
    "cargo build",
    "cargo fmt",
    "提交",
    "开 pr",
    "pull request",
    "cherry-pick",
    "rebase",
    "fix bug",
    "implement ",
    "add feature",
    "apply_patch",
];

const DEFAULT_ARCH: &[&str] = &[
    "重构",
    "架构",
    "隐式状态",
    "技术债",
    "耦合",
    "模块边界",
    "解耦",
    "分层",
    "implicit state",
    "architecture",
    "refactoring strategy",
    "refactor plan",
];

const DEFAULT_CONSULT: &[&str] = &[
    "哪里",
    "哪些",
    "如何",
    "怎么",
    "建议",
    "分析",
    "说明",
    "介绍",
    "严重",
    "痛点",
    "值得",
    "要不要",
    "哪些方面",
    "有何问题",
    "什么问题",
    "where ",
    "what parts",
    "which areas",
    "how should",
    "suggest",
    "recommend",
];

#[inline]
fn contains_any(lower: &str, defaults: &[&str], extras: &[String]) -> bool {
    defaults.iter().any(|k| lower.contains(k))
        || extras
            .iter()
            .any(|k| !k.is_empty() && lower.contains(k.as_str()))
}

/// 是否命中「落地强度」词（改代码/跑构建/提交等）；供 [`super::readonly_overview_bypass`] 复用。
pub fn task_has_impl_strength_markers(lower: &str, extra_blockers: &[String]) -> bool {
    contains_any(lower, DEFAULT_IMPL_STRENGTH, extra_blockers)
}

/// 是否命中咨询/说明类词。
pub fn task_has_consult_markers(lower: &str, extra_markers: &[String]) -> bool {
    contains_any(lower, DEFAULT_CONSULT, extra_markers)
}

/// 在 **`IntentAction::Execute`** 且开启 **`staged_plan_intent_gate_advisory_bypass`** 时：
/// 若命中「架构/咨询」启发式且未命中「落地强度」词，则**绕过分阶段**（由门控返回 [`super::StagedPlanningDenyReason::AdvisoryExecuteBypassStaged`]）。
pub fn should_bypass_staged_for_advisory_execute_task(
    task: &str,
    decision: &IntentDecision,
    staged: &StagedPlanningConfig,
) -> bool {
    if !staged.staged_plan_intent_gate_advisory_bypass {
        return false;
    }
    if !matches!(decision.action, IntentAction::Execute) {
        return false;
    }
    let lower = task.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    if task_has_impl_strength_markers(
        lower.as_str(),
        &staged.staged_plan_advisory_bypass_extra_impl_blockers,
    ) {
        return false;
    }

    let has_arch = lower.contains("隐式")
        || contains_any(
            lower.as_str(),
            DEFAULT_ARCH,
            &staged.staged_plan_advisory_bypass_extra_arch_markers,
        );
    let has_consult = task_has_consult_markers(
        lower.as_str(),
        &staged.staged_plan_advisory_bypass_extra_consult_markers,
    );
    has_arch && has_consult
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::IntentDecision;
    use crate::intent_router::IntentKind;
    use crabmate_config::{StagedPlanBaselineMode, StagedPlanFeedbackMode};

    fn execute_decision() -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: None,
        }
    }

    fn staged_cfg(bypass: bool) -> StagedPlanningConfig {
        StagedPlanningConfig {
            staged_plan_phase_instruction: String::new(),
            staged_plan_allow_no_task: true,
            staged_plan_feedback_mode: StagedPlanFeedbackMode::FailFast,
            staged_plan_patch_max_attempts: 2,
            staged_plan_cli_show_planner_stream: false,
            staged_plan_optimizer_round: false,
            staged_plan_optimizer_requires_parallel_tools: false,
            staged_plan_ensemble_count: 1,
            staged_plan_skip_ensemble_on_casual_prompt: true,
            staged_plan_two_phase_nl_display: false,
            staged_plan_intent_gate_advisory_bypass: bypass,
            staged_plan_baseline_mode: StagedPlanBaselineMode::ImmutableGoalOnly,
            staged_plan_advisory_bypass_extra_impl_blockers: vec![],
            staged_plan_advisory_bypass_extra_arch_markers: vec![],
            staged_plan_advisory_bypass_extra_consult_markers: vec![],
        }
    }

    #[test]
    fn bypass_false_when_bypass_disabled() {
        let s = staged_cfg(false);
        assert!(!should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些耦合问题，请分析",
            &execute_decision(),
            &s,
        ));
    }

    #[test]
    fn bypass_true_for_arch_consult_execute() {
        let s = staged_cfg(true);
        assert!(should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些耦合问题，请分析",
            &execute_decision(),
            &s,
        ));
    }

    #[test]
    fn extra_impl_blocker_prevents_bypass() {
        let mut s = staged_cfg(true);
        s.staged_plan_advisory_bypass_extra_impl_blockers = vec!["请落地".to_string()];
        assert!(!should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些问题，请分析请落地改造",
            &execute_decision(),
            &s,
        ));
    }

    #[test]
    fn extra_arch_marker_enables_bypass() {
        let mut s = staged_cfg(true);
        s.staged_plan_advisory_bypass_extra_arch_markers = vec!["microservice".to_string()];
        assert!(should_bypass_staged_for_advisory_execute_task(
            "microservice 边界有哪些风险，建议怎么拆",
            &execute_decision(),
            &s,
        ));
    }
}
