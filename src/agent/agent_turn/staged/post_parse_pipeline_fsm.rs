//! 首轮 **`agent_reply_plan` 解析成功且非 `no_task`** 之后、**步入执行循环之前** 的纯编排表：
//! ensemble 合并是否调用、是否按「寒暄启发式」跳过、优化轮是否运行，以及结构化 `debug!` 文案。
//! **不**发起 LLM；与 `planner_round_fsm`（路由计算）配合使用。

use log::debug;

use super::planner_round_fsm::{StagedPlanEnsembleRoute, StagedPlanOptimizerRoute};

/// 是否应调用 `maybe_run_staged_plan_ensemble_then_merge`（与 `SkipValidateOnlyBinding` 时整段跳过等价）。
#[inline]
pub(crate) fn ensemble_merge_should_invoke(route: StagedPlanEnsembleRoute) -> bool {
    !matches!(route, StagedPlanEnsembleRoute::SkipValidateOnlyBinding)
}

/// 传入 `maybe_run_staged_plan_ensemble_then_merge` 的 `skip_for_casual_user_prompt`（仅当 `ensemble_merge_should_invoke` 为真时有意义）。
#[inline]
pub(crate) fn ensemble_merge_skip_for_casual_prompt(route: StagedPlanEnsembleRoute) -> bool {
    matches!(route, StagedPlanEnsembleRoute::SkipCasualHeuristic)
}

/// 是否应跑优化轮 LLM（与 `staged_plan_optimizer_route == Run` 等价）。
#[inline]
pub(crate) fn optimizer_round_should_run(route: StagedPlanOptimizerRoute) -> bool {
    matches!(route, StagedPlanOptimizerRoute::Run)
}

pub(crate) fn log_staged_plan_ensemble_route(
    route: StagedPlanEnsembleRoute,
    staged_plan_ensemble_count: u8,
) {
    match route {
        StagedPlanEnsembleRoute::SkipValidateOnlyBinding => {
            debug!(
                target: "crabmate",
                "分阶段规划·逻辑多规划员：检测到 workflow_validate_only 节点绑定上下文，跳过 ensemble 以保持逐步绑定稳定"
            );
        }
        StagedPlanEnsembleRoute::SkipCasualHeuristic => {
            debug!(
                target: "crabmate",
                "分阶段规划·逻辑多规划员：用户输入偏短/寒暄启发式，跳过 ensemble（staged_plan_ensemble_count={}）以省 API",
                staged_plan_ensemble_count
            );
        }
        StagedPlanEnsembleRoute::SkipNotConfigured | StagedPlanEnsembleRoute::Run => {}
    }
}

pub(crate) fn log_staged_plan_optimizer_route(
    route: StagedPlanOptimizerRoute,
    plan_steps_len: usize,
) {
    match route {
        StagedPlanOptimizerRoute::SkipValidateOnlyBinding => {
            debug!(
                target: "crabmate",
                "分阶段规划优化轮：检测到 workflow_validate_only 节点绑定上下文，跳过优化轮以避免破坏绑定约束"
            );
        }
        StagedPlanOptimizerRoute::SkipNoParallelTools => {
            debug!(
                target: "crabmate",
                "分阶段规划优化轮：本会话无可同轮并行批处理的内建工具，跳过优化轮以省 API（步数={}）",
                plan_steps_len
            );
        }
        StagedPlanOptimizerRoute::SkipStepsLt2
        | StagedPlanOptimizerRoute::SkipOptimizerRoundDisabled
        | StagedPlanOptimizerRoute::Run => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensemble_invoke_skips_only_validate_binding() {
        assert!(!ensemble_merge_should_invoke(
            StagedPlanEnsembleRoute::SkipValidateOnlyBinding
        ));
        assert!(ensemble_merge_should_invoke(StagedPlanEnsembleRoute::Run));
        assert!(ensemble_merge_should_invoke(
            StagedPlanEnsembleRoute::SkipCasualHeuristic
        ));
    }

    #[test]
    fn casual_skip_only_on_heuristic_route() {
        assert!(ensemble_merge_skip_for_casual_prompt(
            StagedPlanEnsembleRoute::SkipCasualHeuristic
        ));
        assert!(!ensemble_merge_skip_for_casual_prompt(
            StagedPlanEnsembleRoute::Run
        ));
    }

    #[test]
    fn optimizer_run_only_when_run_variant() {
        assert!(optimizer_round_should_run(StagedPlanOptimizerRoute::Run));
        assert!(!optimizer_round_should_run(
            StagedPlanOptimizerRoute::SkipStepsLt2
        ));
    }

    #[test]
    fn log_routes_do_not_panic() {
        log_staged_plan_optimizer_route(StagedPlanOptimizerRoute::SkipNoParallelTools, 0);
        log_staged_plan_ensemble_route(StagedPlanEnsembleRoute::SkipNotConfigured, 1);
    }
}
