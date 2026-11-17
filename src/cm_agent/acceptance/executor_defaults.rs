//! 解析期给 `test_runner` 步注入缺省 `expect_exit_code`。
//!
//! **#1429**：`effective_plan_step_acceptance`（分阶段步合并）与
//! `default_exit_code_for_build_execution_description`（分层子目标按描述注入）已删除——
//! 两者的消费者随 `chore(staged-removal)` 消失，只剩 golden 与单测引用。
//! 余下的 [`apply_executor_kind_acceptance_defaults`] 由 `plan_artifact/parse.rs` 在生产路径调用。

use crate::cm_agent::plan_artifact::{PlanStepAcceptance, PlanStepExecutorKind, PlanStepV1};

/// `test_runner` 子目标缺省期望退出码。
pub const DEFAULT_COMMAND_EXIT_CODE: i32 = 0;

/// 解析后对齐 `executor_kind` 仍缺省时，注入确定性验收字段（就地修改 `step`）。
pub fn apply_executor_kind_acceptance_defaults(step: &mut PlanStepV1) {
    if step.executor_kind != Some(PlanStepExecutorKind::TestRunner) {
        return;
    }
    let acc = step
        .acceptance
        .get_or_insert_with(PlanStepAcceptance::default);
    if acc.expect_exit_code.is_none() {
        acc.expect_exit_code = Some(DEFAULT_COMMAND_EXIT_CODE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_gets_default_exit_code_when_acceptance_missing() {
        let mut step = PlanStepV1 {
            id: "t".into(),
            description: "cargo test".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::TestRunner),
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        apply_executor_kind_acceptance_defaults(&mut step);
        let acc = step.acceptance.expect("injected");
        assert_eq!(acc.expect_exit_code, Some(0));
    }

    /// 非 `test_runner` 不应被注入缺省验收（覆盖 `apply_executor_kind_acceptance_defaults` 的 early-return）。
    #[test]
    fn review_readonly_is_left_untouched() {
        let mut step = PlanStepV1 {
            id: "r".into(),
            description: "read docs".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        apply_executor_kind_acceptance_defaults(&mut step);
        assert!(step.acceptance.is_none(), "非 test_runner 不应注入缺省验收");
    }
}
