//! 分阶段 **`staged_plan_intent_gate_advisory_bypass`** 用的「咨询类 Execute → 绕过分阶段」启发式。
//!
//! 内置中英文关键词表可经 **[`crate::config::StagedPlanningConfig`]** 中三个 `*_extra_*` 列表**追加**（运行时一律小写匹配）。

use crate::intent_pipeline::{IntentAction, IntentDecision};

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
    advisory_bypass_enabled: bool,
    extra_impl_blockers: &[String],
    extra_arch_markers: &[String],
    extra_consult_markers: &[String],
) -> bool {
    if !advisory_bypass_enabled {
        return false;
    }
    if !matches!(decision.action, IntentAction::Execute) {
        return false;
    }
    let lower = task.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    if task_has_impl_strength_markers(lower.as_str(), extra_impl_blockers) {
        return false;
    }

    let has_arch =
        lower.contains("隐式") || contains_any(lower.as_str(), DEFAULT_ARCH, extra_arch_markers);
    let has_consult = task_has_consult_markers(lower.as_str(), extra_consult_markers);
    has_arch && has_consult
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::IntentDecision;
    use crate::intent_router::IntentKind;

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

    #[test]
    fn bypass_false_when_bypass_disabled() {
        assert!(!should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些耦合问题，请分析",
            &execute_decision(),
            false,            // advisory_bypass_enabled
            &[] as &[String], // extra_impl_blockers
            &[] as &[String], // extra_arch_markers
            &[] as &[String], // extra_consult_markers
        ));
    }

    #[test]
    fn bypass_true_for_arch_consult_execute() {
        assert!(should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些耦合问题，请分析",
            &execute_decision(),
            true, // advisory_bypass_enabled
            &[] as &[String],
            &[] as &[String],
            &[] as &[String],
        ));
    }

    #[test]
    fn extra_impl_blocker_prevents_bypass() {
        assert!(!should_bypass_staged_for_advisory_execute_task(
            "架构上有哪些问题，请分析请落地改造",
            &execute_decision(),
            true, // advisory_bypass_enabled
            &["请落地".to_string()],
            &[] as &[String],
            &[] as &[String],
        ));
    }

    #[test]
    fn extra_arch_marker_enables_bypass() {
        assert!(should_bypass_staged_for_advisory_execute_task(
            "microservice 边界有哪些风险，建议怎么拆",
            &execute_decision(),
            true, // advisory_bypass_enabled
            &[] as &[String],
            &["microservice".to_string()],
            &[] as &[String],
        ));
    }
}
