//! 单次分阶段「规划子回合」内 **首轮 JSON 解析成功之后** 的门控：ensemble / 优化轮是否运行。
//! 纯函数表驱动，便于单测与日志对齐；**不**发起 LLM（调用点仍在 `mod.rs`）。

/// 逻辑多规划员（ensemble）是否在本轮执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanEnsembleRoute {
    /// `staged_plan_ensemble_count <= 1`，等价于关闭。
    SkipNotConfigured,
    /// `workflow_validate_only` 绑定上下文：保持逐步绑定稳定。
    SkipValidateOnlyBinding,
    /// 寒暄/极短启发式，省 API。
    SkipCasualHeuristic,
    /// 调用 `maybe_run_staged_plan_ensemble_then_merge`。
    Run,
}

/// 规划步骤优化轮是否在本轮执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanOptimizerRoute {
    SkipStepsLt2,
    SkipOptimizerRoundDisabled,
    SkipValidateOnlyBinding,
    SkipNoParallelTools,
    Run,
}

/// 与 `maybe_run_staged_plan_ensemble_then_merge` 前的 `if` 链等价。
pub(crate) fn staged_plan_ensemble_route(
    staged_plan_ensemble_count: u8,
    staged_plan_skip_ensemble_on_casual_prompt: bool,
    validate_only_binding_active: bool,
    trigger_user_content: Option<&str>,
) -> StagedPlanEnsembleRoute {
    if staged_plan_ensemble_count <= 1 {
        return StagedPlanEnsembleRoute::SkipNotConfigured;
    }
    if validate_only_binding_active {
        return StagedPlanEnsembleRoute::SkipValidateOnlyBinding;
    }
    if staged_plan_skip_ensemble_on_casual_prompt
        && let Some(t) = trigger_user_content
        && crate::agent::plan_optimizer::staged_plan_user_prompt_looks_like_casual_or_trivial(t)
    {
        return StagedPlanEnsembleRoute::SkipCasualHeuristic;
    }
    StagedPlanEnsembleRoute::Run
}

/// 与优化轮 `if want_optimizer && !skip_*` 等价。
pub(crate) fn staged_plan_optimizer_route(
    plan_steps_len: usize,
    staged_plan_optimizer_round: bool,
    validate_only_binding_active: bool,
    staged_plan_optimizer_requires_parallel_tools: bool,
    parallel_tool_names_csv: &str,
) -> StagedPlanOptimizerRoute {
    if plan_steps_len < 2 {
        return StagedPlanOptimizerRoute::SkipStepsLt2;
    }
    if !staged_plan_optimizer_round {
        return StagedPlanOptimizerRoute::SkipOptimizerRoundDisabled;
    }
    if validate_only_binding_active {
        return StagedPlanOptimizerRoute::SkipValidateOnlyBinding;
    }
    if staged_plan_optimizer_requires_parallel_tools && parallel_tool_names_csv.trim().is_empty() {
        return StagedPlanOptimizerRoute::SkipNoParallelTools;
    }
    StagedPlanOptimizerRoute::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensemble_skips_when_count_one() {
        assert_eq!(
            staged_plan_ensemble_route(1, true, false, Some("hello")),
            StagedPlanEnsembleRoute::SkipNotConfigured
        );
    }

    #[test]
    fn ensemble_skips_binding_context() {
        assert_eq!(
            staged_plan_ensemble_route(3, false, true, Some("task")),
            StagedPlanEnsembleRoute::SkipValidateOnlyBinding
        );
    }

    #[test]
    fn ensemble_skips_casual_when_heuristic_matches() {
        assert_eq!(
            staged_plan_ensemble_route(3, true, false, Some("谢谢")),
            StagedPlanEnsembleRoute::SkipCasualHeuristic
        );
    }

    #[test]
    fn ensemble_runs_when_multi_and_not_skipped() {
        assert_eq!(
            staged_plan_ensemble_route(
                3,
                true,
                false,
                Some("请在本仓库完整修复编译错误并运行 cargo test 验证"),
            ),
            StagedPlanEnsembleRoute::Run
        );
    }

    #[test]
    fn optimizer_skips_single_step() {
        assert_eq!(
            staged_plan_optimizer_route(1, true, false, true, "read_file"),
            StagedPlanOptimizerRoute::SkipStepsLt2
        );
    }

    #[test]
    fn optimizer_skips_when_disabled() {
        assert_eq!(
            staged_plan_optimizer_route(3, false, false, true, "read_file"),
            StagedPlanOptimizerRoute::SkipOptimizerRoundDisabled
        );
    }

    #[test]
    fn optimizer_skips_parallel_when_csv_empty_and_gate_on() {
        assert_eq!(
            staged_plan_optimizer_route(3, true, false, true, "  \n"),
            StagedPlanOptimizerRoute::SkipNoParallelTools
        );
    }

    #[test]
    fn optimizer_runs_when_parallel_csv_nonempty() {
        assert_eq!(
            staged_plan_optimizer_route(3, true, false, true, "read_file,list_dir"),
            StagedPlanOptimizerRoute::Run
        );
    }
}
