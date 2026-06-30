//! 分层子目标验收：与分阶段 [`effective_plan_step_acceptance`] 对齐的缺省与有效性判定。

use crate::agent::acceptance::{
    VerifyOutcome, default_exit_code_for_build_execution_description,
    parse_exit_code_from_combined_output,
};

use super::task::{GoalAcceptance, SubGoal, TaskResult};

impl GoalAcceptance {
    /// 是否含至少一条可执行验收规则（含 `expect_command_success`）。
    pub fn is_effective(&self) -> bool {
        !self.to_acceptance_spec().is_empty()
            || self
                .expect_command_success
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
    }
}

/// 分层子目标验收用的**有效** `acceptance`：合并模型字段与构建类描述缺省（不修改 `goal`）。
pub fn effective_goal_acceptance(goal: &SubGoal) -> Option<GoalAcceptance> {
    let mut merged = goal.acceptance.clone().unwrap_or_default();
    if merged.expect_exit_code.is_none()
        && let Some(code) =
            default_exit_code_for_build_execution_description(goal.description.as_str())
    {
        merged.expect_exit_code = Some(code);
    }
    if merged.is_effective() {
        Some(merged)
    } else {
        None
    }
}

/// 对 [`GoalAcceptance`] 规范在 [`TaskResult`] 上执行共用内核判定（不含 `expect_command_success` 二次命令）。
pub fn verify_goal_acceptance_spec(
    acceptance: &GoalAcceptance,
    result: &TaskResult,
    workspace_root: &std::path::Path,
) -> VerifyOutcome {
    let spec = acceptance.to_acceptance_spec();
    if spec.is_empty() {
        return VerifyOutcome::Pass;
    }
    let output = result.output.as_deref().unwrap_or("");
    let error = result.error.as_deref().unwrap_or("");
    let combined = format!("{output} {error}");
    let exit_parsed = parse_exit_code_from_combined_output(&combined);
    let tool_for_http = result
        .tools_invoked
        .last()
        .map(|s| s.as_str())
        .filter(|n| {
            let l = n.to_lowercase();
            l.contains("http") || l.contains("fetch")
        })
        .unwrap_or("");
    let ev = crate::agent::acceptance::AcceptanceEvidence {
        tool_name: tool_for_http,
        tool_output: combined.as_str(),
        stdout: output,
        stderr: error,
        tool_error: None,
        fallback_exit_code: exit_parsed,
        workspace_root,
        file_resolve: spec.file_resolve,
        combined_text_override: Some(combined.as_str()),
    };
    crate::agent::acceptance::verify_against_spec(&spec, &ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::hierarchy::task::SubGoal;

    #[test]
    fn effective_goal_injects_build_exit_code() {
        let goal = SubGoal::new("g1", "在工作区运行 cargo build");
        let eff = effective_goal_acceptance(&goal).expect("effective");
        assert_eq!(eff.expect_exit_code, Some(0));
        assert!(goal.acceptance.is_none());
    }

    #[test]
    fn effective_goal_none_for_readonly_description() {
        let goal = SubGoal::new("g2", "阅读 README 概览");
        assert!(effective_goal_acceptance(&goal).is_none());
    }
}
