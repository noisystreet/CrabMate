//! 分阶段 `executor_kind` 与分层子目标共用的**验收默认值**（模型未填 `acceptance` 时由编排层补齐）。
//!
//! 与 [`crate::plan_artifact::align_plan_step_acceptance_with_executor_kind`]（剥离不匹配字段）配合：
//! 先对齐/剥离，再注入缺省规则，避免双轨（`staged` / `hierarchy`）各自维护一套启发式。

use crate::plan_artifact::{
    PlanStepAcceptance, PlanStepExecutorKind, PlanStepV1,
    plan_step_description_implies_build_execution,
};

/// `test_runner` / 构建类子目标缺省期望退出码（与 `step_verifier` 缺省策略一致）。
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

/// 分阶段步验收用的**有效** `acceptance`：合并模型字段与 `executor_kind` 缺省（不修改原 `step`）。
pub fn effective_plan_step_acceptance(step: &PlanStepV1) -> Option<PlanStepAcceptance> {
    let mut merged = step.acceptance.clone().unwrap_or_default();
    if step.executor_kind == Some(PlanStepExecutorKind::TestRunner)
        && merged.expect_exit_code.is_none()
    {
        merged.expect_exit_code = Some(DEFAULT_COMMAND_EXIT_CODE);
    }
    if merged.is_effective() {
        Some(merged)
    } else {
        None
    }
}

/// 分层子目标：描述像构建/测试执行且未显式给出 `acceptance` 时的缺省退出码。
pub fn default_exit_code_for_build_execution_description(desc: &str) -> Option<i32> {
    if plan_step_description_implies_build_execution(desc) {
        Some(DEFAULT_COMMAND_EXIT_CODE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_artifact::PlanStepV1;

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

    #[test]
    fn effective_plan_step_acceptance_merges_without_mutation() {
        let step = PlanStepV1 {
            id: "t".into(),
            description: "run tests".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::TestRunner),
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        let eff = effective_plan_step_acceptance(&step).expect("effective");
        assert_eq!(eff.expect_exit_code, Some(0));
        assert!(step.acceptance.is_none());
    }

    #[test]
    fn review_readonly_without_acceptance_stays_none() {
        let step = PlanStepV1 {
            id: "r".into(),
            description: "read docs".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        assert!(effective_plan_step_acceptance(&step).is_none());
    }

    #[test]
    fn build_description_default_exit_code() {
        assert_eq!(
            default_exit_code_for_build_execution_description("在本工作区运行 cargo build"),
            Some(0)
        );
        assert!(default_exit_code_for_build_execution_description("阅读 README").is_none());
    }
}
