//! 在 `run_agent_turn` 起点的**意图门控**（可选；由 `intent_at_turn_start_enabled` 控制）：
//! 门控开启且会话为 Act 时仅跑**廉价启发式**（用户句执行约束关键词）；**不再**调用 L2 LLM，
//! **不再**发送 `intent_analysis` SSE。门控关闭或 Ask/Plan 时早退（只读由 `run_dispatch` / mode 决定）。

use crate::agent::plan_artifact::PlanStepExecutorKind;
use crabmate_agent::agent_turn::IntentGateSnapshot;
use crabmate_types::SessionMode;

use super::intent_user;
use crate::agent::agent_turn::params::RunLoopParams;

/// 门控关，或 Ask/Plan（能力档已由 mode 决定）时跳过门控启发式。
#[must_use]
fn should_skip_turn_start_heuristics(gate_enabled: bool, session_mode: SessionMode) -> bool {
    !gate_enabled || crate::session_mode_turn::session_mode_requires_readonly_tools(session_mode)
}

/// `false` 表示本回合已写入助手终答，调用方应结束本回合。
/// R2 起门控路径恒为继续主执行（仅可能挂只读约束），故恒返回 `true`（空任务 / 跳过亦同）。
pub(crate) fn run_intent_at_turn_start_if_configured(
    p: &mut RunLoopParams<'_>,
) -> Result<bool, crate::agent::agent_turn::errors::RunAgentTurnError> {
    let in_clarification_flow =
        intent_user::recently_waiting_execute_confirmation(p.turn.messages());
    let task = intent_user::extract_effective_user_task(p.turn.messages(), in_clarification_flow);
    if task.trim().is_empty() {
        p.turn.turn_planner_hints.intent_gate_snapshot = Some(IntentGateSnapshot::EmptyTask);
        return Ok(true);
    }

    let gate_enabled = p.ctx.core.cfg.intent_routing.intent_at_turn_start_enabled;
    let session_mode = p.ctx.attach.session_mode;
    if should_skip_turn_start_heuristics(gate_enabled, session_mode) {
        p.turn.turn_planner_hints.intent_gate_snapshot = Some(IntentGateSnapshot::Disabled);
        return Ok(true);
    }

    apply_gate_on_act_heuristics(p, &task);
    Ok(true)
}

/// 门控开 + Act：关键词执行约束；观测快照为固定 `ProceedExecute`（无 L2 / 无旁注 SSE）。
fn apply_gate_on_act_heuristics(p: &mut RunLoopParams<'_>, task: &str) {
    if let Some(constraints) = infer_turn_execution_constraints(task)
        && constraints.requires_review_readonly()
    {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(PlanStepExecutorKind::ReviewReadonly);
        p.turn.turn_planner_hints.intent_turn_gate_hint = Some(constraints.intent_gate_hint_zh());
    }
    p.turn.turn_planner_hints.intent_gate_snapshot = Some(IntentGateSnapshot::ProceedExecute {
        kind: "execute".to_string(),
        primary_intent: "gate.heuristics".to_string(),
        action: "execute".to_string(),
        confidence: 1.0,
        need_clarification: false,
    });
}

#[cfg(test)]
mod skip_heuristics_tests {
    use super::should_skip_turn_start_heuristics;
    use crabmate_types::SessionMode;

    #[test]
    fn skips_when_gate_off() {
        assert!(should_skip_turn_start_heuristics(false, SessionMode::Act));
    }

    #[test]
    fn skips_ask_plan_even_when_gate_on() {
        assert!(should_skip_turn_start_heuristics(true, SessionMode::Ask));
        assert!(should_skip_turn_start_heuristics(true, SessionMode::Plan));
    }

    #[test]
    fn runs_heuristics_for_act_when_gate_on() {
        assert!(!should_skip_turn_start_heuristics(true, SessionMode::Act));
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

    fn intent_gate_hint_zh(self) -> String {
        let mut limits = Vec::new();
        if self.no_write {
            limits.push("不得修改文件、不得继续 patch");
        }
        if self.no_command_execution {
            limits.push("不得运行构建/测试/执行类命令");
        }
        if self.analysis_only {
            limits.push("以只读诊断、原因分析和操作说明为主");
        }
        if self.ask_before_mutation {
            limits.push("如需再次执行或修改，必须先说明原因并取得用户确认");
        }
        format!(
            "【意图门控】用户本轮给出了执行约束：{}。当前回合按只读诊断处理：可以读取/列目录/解释失败原因，但不要越过上述约束。",
            limits.join("；")
        )
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

    /// R2 表征：开门控 + Act 时，普通写/修请求**不**因「无 L2」挂只读。
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
                "gate ON + Act must not blanket-narrow for task={task:?}"
            );
        }
    }

    /// 仅「不要修改」而无分析/解释类词时，不满足 `requires_review_readonly`（覆盖面窄于旧 fail-open）。
    #[test]
    fn no_write_alone_does_not_require_review_readonly() {
        let c = infer_turn_execution_constraints("不要修改文件").expect("constraints");
        assert!(c.no_write);
        assert!(!c.requires_review_readonly());
    }
}
