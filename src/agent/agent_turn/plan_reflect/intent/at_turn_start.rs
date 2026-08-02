//! 在 `run_agent_turn` 起点的 **Act 句关键词启发式**（无额外 LLM）：
//! Ask/Plan 跳过（只读由 `run_dispatch` / session_mode 决定）；Act 常跑执行约束关键词收窄。
//!
//! 启发式直接看最新真实 user 句（不改写「继续」/确认续接为前序任务）。

use crate::agent::plan_artifact::PlanStepExecutorKind;
use crabmate_agent::agent_turn::TurnStartSnapshot;
use crabmate_types::SessionMode;

use crate::agent::agent_turn::params::RunLoopParams;

/// Ask/Plan 由 mode 决定只读档，跳过 Act 句启发式。
#[must_use]
fn should_skip_act_utterance_heuristics(session_mode: SessionMode) -> bool {
    crate::session_mode_turn::session_mode_requires_readonly_tools(session_mode)
}

/// 最新真实用户任务句（跳过编排注入）；供 Act 关键词启发式使用。
fn latest_user_task_for_heuristics(messages: &[crabmate_types::Message]) -> String {
    crabmate_types::last_real_user_task_content(messages, false)
        .unwrap_or_default()
        .to_string()
}

/// 回合起点启发式：恒继续主执行（仅可能挂只读约束）。
pub(crate) fn run_act_turn_start_heuristics(p: &mut RunLoopParams<'_>) {
    let task = latest_user_task_for_heuristics(p.turn.messages());
    if task.trim().is_empty() {
        p.turn.turn_planner_hints.turn_start_snapshot = Some(TurnStartSnapshot::EmptyTask);
        return;
    }

    if should_skip_act_utterance_heuristics(p.ctx.attach.session_mode) {
        p.turn.turn_planner_hints.turn_start_snapshot = Some(TurnStartSnapshot::Disabled);
        return;
    }

    apply_act_utterance_heuristics(p, &task);
}

fn apply_act_utterance_heuristics(p: &mut RunLoopParams<'_>, task: &str) {
    let mut review_readonly = false;
    if let Some(constraints) = infer_turn_execution_constraints(task)
        && constraints.requires_review_readonly()
    {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(PlanStepExecutorKind::ReviewReadonly);
        p.turn.turn_planner_hints.execution_constraint_hint =
            Some(constraints.execution_constraint_hint_zh());
        review_readonly = true;
    }
    p.turn.turn_planner_hints.turn_start_snapshot =
        Some(TurnStartSnapshot::ActHeuristics { review_readonly });
}

#[cfg(test)]
mod skip_heuristics_tests {
    use super::should_skip_act_utterance_heuristics;
    use crabmate_types::SessionMode;

    #[test]
    fn skips_ask_plan() {
        assert!(should_skip_act_utterance_heuristics(SessionMode::Ask));
        assert!(should_skip_act_utterance_heuristics(SessionMode::Plan));
    }

    #[test]
    fn runs_for_act() {
        assert!(!should_skip_act_utterance_heuristics(SessionMode::Act));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TurnExecutionConstraints {
    no_write: bool,
    no_command_execution: bool,
    analysis_only: bool,
    ask_before_mutation: bool,
}

impl TurnExecutionConstraints {
    fn requires_review_readonly(self) -> bool {
        self.analysis_only && (self.no_write || self.no_command_execution)
    }

    /// 本轮短约束（L7）：只列用户句命中的限制，勿复述 mode / base 的全局原则。
    fn execution_constraint_hint_zh(self) -> String {
        let mut limits = Vec::new();
        if self.no_write {
            limits.push("不得改文件/patch");
        }
        if self.no_command_execution {
            limits.push("不得跑构建/测试/执行类命令");
        }
        if self.analysis_only {
            limits.push("只读诊断");
        }
        if self.ask_before_mutation {
            limits.push("再改须先征得确认");
        }
        format!("【执行约束】本轮：{}。勿越过。", limits.join("；"))
    }
}

fn infer_turn_execution_constraints(task: &str) -> Option<TurnExecutionConstraints> {
    let t = task.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let no_write = [
        "取消修复",
        "不用修复",
        "不要修复",
        "别修复",
        "不要修改",
        "别修改",
        "先别改",
        "先不要改",
        "不改代码",
        "without modifying",
        "do not modify",
        "don't modify",
        "no patch",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let no_command_execution = [
        "不要运行",
        "别运行",
        "不要执行",
        "别执行",
        "不要编译",
        "别编译",
        "不要跑",
        "别跑",
        "do not run",
        "don't run",
        "without running",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let analysis_only = [
        "分析",
        "诊断",
        "说明",
        "解释",
        "只解释",
        "仅解释",
        "怎么编译",
        "如何编译",
        "怎么做",
        "只读",
        "只分析",
        "analyze",
        "diagnose",
        "explain",
        "how to",
        "readonly",
        "read-only",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let ask_before_mutation = no_write || t.contains("先问我") || t.contains("先确认");
    let constraints = TurnExecutionConstraints {
        no_write,
        no_command_execution: no_command_execution || (no_write && analysis_only),
        analysis_only,
        ask_before_mutation,
    };
    (constraints != TurnExecutionConstraints::default()).then_some(constraints)
}

#[cfg(test)]
mod tests {
    use super::infer_turn_execution_constraints;

    #[test]
    fn infers_readonly_constraints_from_cancel_fix_analyze_request() {
        let c = infer_turn_execution_constraints(
            "应该不用修改就可以编译，先取消修复，然后分析一下怎么编译",
        )
        .expect("constraints");
        assert!(c.no_write);
        assert!(c.no_command_execution);
        assert!(c.analysis_only);
        assert!(c.requires_review_readonly());

        let c =
            infer_turn_execution_constraints("不要修改文件，只分析失败原因").expect("constraints");
        assert!(c.requires_review_readonly());
    }

    #[test]
    fn infers_command_execution_constraint() {
        let c = infer_turn_execution_constraints("先不要运行测试，只分析一下失败原因")
            .expect("constraints");
        assert!(c.no_command_execution);
        assert!(c.analysis_only);
        assert!(c.requires_review_readonly());
    }

    #[test]
    fn does_not_mark_plain_build_request_readonly() {
        assert!(infer_turn_execution_constraints("编译 hpcg").is_none());
        let c = infer_turn_execution_constraints("分析当前项目").expect("analysis constraint");
        assert!(c.analysis_only);
        assert!(!c.requires_review_readonly());
    }

    #[test]
    fn explain_only_without_execute_marks_readonly() {
        let c = infer_turn_execution_constraints("不要执行，只解释这个错误").expect("constraints");
        assert!(c.no_command_execution);
        assert!(c.analysis_only);
        assert!(c.requires_review_readonly());
    }

    #[test]
    fn plain_act_style_requests_do_not_force_review_readonly() {
        for task in [
            "帮我修复这个报错",
            "编译 hpcg",
            "提交一个 pull request",
            "分析当前项目",
        ] {
            let readonly = infer_turn_execution_constraints(task)
                .is_some_and(|c| c.requires_review_readonly());
            assert!(
                !readonly,
                "Act heuristics must not blanket-narrow for task={task:?}"
            );
        }
    }

    #[test]
    fn no_write_alone_does_not_require_review_readonly() {
        let c = infer_turn_execution_constraints("不要修改文件").expect("constraints");
        assert!(c.no_write);
        assert!(!c.requires_review_readonly());
    }

    #[test]
    fn execution_constraint_hint_is_short_and_turn_scoped() {
        use super::TurnExecutionConstraints;
        let hint = TurnExecutionConstraints {
            no_write: true,
            no_command_execution: true,
            analysis_only: true,
            ask_before_mutation: true,
        }
        .execution_constraint_hint_zh();
        assert!(hint.starts_with("【执行约束】本轮："));
        assert!(hint.contains("不得改文件/patch"));
        assert!(hint.contains("不得跑构建/测试/执行类命令"));
        assert!(hint.contains("只读诊断"));
        assert!(hint.contains("再改须先征得确认"));
        assert!(hint.ends_with("勿越过。"));
        // 勿复述 mode_act / 旧模板中的全局只读叙事
        assert!(!hint.contains("可以读取"));
        assert!(!hint.contains("列目录"));
        assert!(!hint.contains("用户本轮给出了限制"));
        assert!(
            hint.chars().count() <= 120,
            "hint should stay short, got {} chars: {hint}",
            hint.chars().count()
        );
    }

    #[test]
    fn latest_user_task_skips_orchestration_injection() {
        use super::latest_user_task_for_heuristics;
        let messages = vec![
            crabmate_types::Message::user_only("编译 hpcg"),
            crabmate_types::Message::user_only("【编排纠偏】继续构建"),
        ];
        assert_eq!(latest_user_task_for_heuristics(&messages), "编译 hpcg");
    }
}
