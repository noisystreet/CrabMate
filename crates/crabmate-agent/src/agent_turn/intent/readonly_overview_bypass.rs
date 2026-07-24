//! 只读「项目/仓库概览」类 **Execute** 任务：绕过分阶段无工具规划轮，改走单 Agent 外循环（可调用只读工具），
//! 避免规划轮已写出长文 Markdown 却因缺 `agent_reply_plan` JSON 再降级外循环导致重复终答。

use crate::intent_pipeline::{IntentAction, IntentDecision};
use crate::intent_router::{qa_explain_style_primary, qa_readonly_style_primary};

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
fn contains_any(lower: &str, defaults: &[&str], extras: &[&str]) -> bool {
    defaults.iter().any(|k| lower.contains(k))
        || extras.iter().any(|k| !k.is_empty() && lower.contains(k))
}

fn task_has_impl_strength_markers(lower: &str, extra_blockers: &[&str]) -> bool {
    contains_any(lower, DEFAULT_IMPL_STRENGTH, extra_blockers)
}

fn task_has_consult_markers(lower: &str, extra_markers: &[&str]) -> bool {
    contains_any(lower, DEFAULT_CONSULT, extra_markers)
}

const OVERVIEW_SCOPE_MARKERS: &[&str] = &[
    "项目",
    "仓库",
    "代码库",
    "代码结构",
    "目录结构",
    "当前工程",
    "本仓库",
    "repo",
    "codebase",
    "project structure",
    "this project",
    "the repo",
];

/// `primary_intent` 为只读探查/概览类（与 L2 对齐）。
#[inline]
pub fn readonly_overview_style_primary(primary_intent: &str) -> bool {
    primary_intent == "execute.read_inspect"
        || primary_intent.starts_with("execute.read_inspect.")
        || qa_readonly_style_primary(primary_intent)
        || qa_explain_style_primary(primary_intent)
}

/// 用户句是否像「分析/介绍当前项目或仓库」且未命中落地强度词。
pub fn readonly_overview_task_heuristic(task: &str) -> bool {
    let lower = task.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if task_has_impl_strength_markers(&lower, &[]) {
        return false;
    }
    let has_consult = task_has_consult_markers(&lower, &[]);
    let has_scope = OVERVIEW_SCOPE_MARKERS.iter().any(|m| lower.contains(m));
    has_consult && has_scope
}

/// 非分层门控：此类 Execute 不进入滚动分阶段规划。
pub fn should_bypass_staged_for_readonly_overview_execute(
    task: &str,
    decision: &IntentDecision,
) -> bool {
    if !matches!(decision.action, IntentAction::Execute) {
        return false;
    }
    let lower = task.trim().to_lowercase();
    if task_has_impl_strength_markers(&lower, &[]) {
        return false;
    }
    if readonly_overview_style_primary(decision.primary_intent.as_str()) {
        return true;
    }
    readonly_overview_task_heuristic(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;

    fn execute_decision(primary: &str) -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: primary.to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: None,
        }
    }

    #[test]
    fn read_inspect_primary_bypasses_staged() {
        assert!(should_bypass_staged_for_readonly_overview_execute(
            "分析当前项目",
            &execute_decision("execute.read_inspect"),
        ));
    }

    #[test]
    fn analyze_current_project_heuristic_bypasses() {
        assert!(should_bypass_staged_for_readonly_overview_execute(
            "分析当前项目",
            &execute_decision("execute.code_change"),
        ));
    }

    #[test]
    fn impl_task_does_not_bypass() {
        assert!(!should_bypass_staged_for_readonly_overview_execute(
            "分析当前项目并请修改 main.rs",
            &execute_decision("execute.read_inspect"),
        ));
    }
}
