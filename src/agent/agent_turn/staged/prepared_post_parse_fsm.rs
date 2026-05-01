//! **`resolve_parse_with_assistant`** 成功之后、步循环之前的 **post-parse 调度**（无 IO）：
//! **`no_task`** 与 **结构化 steps** 两条路径分叉，避免 `mod.rs` 中重复的 **`if plan.no_task`** 与后续 ensemble 块交织。
//!
//! **`PreparedFullPipelineSchedule`**：在 **`FullPipelineThenSteps`** 路径上一次性汇总 **ensemble / 优化 / 两阶段 NL** 的路由结果，
//! 避免 `run_staged_plan_with_prepared_request` 内多处重复读取 **`validate_only_binding`** 与分散的 **`if`**。
//!
//! 与 **`planner_round_fsm`** / **`post_parse_pipeline_fsm`**（ensemble/优化 **是否运行**）正交（本模块调用前者并持有返回值）。

use super::planner_round_fsm::{
    StagedPlanEnsembleRoute, StagedPlanOptimizerRoute, staged_plan_ensemble_route,
    staged_plan_optimizer_route,
};

/// 首轮解析成功后，后续子阶段的粗粒度顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreparedPostParseSchedule {
    /// **`no_task=true`**：可选两阶段 NL → **`run_agent_outer_loop`**（**不**跑 ensemble/优化/分步循环）。
    NoTaskThenOuter,
    /// 结构化规划：ensemble（若路由允许）→ 优化轮（若路由允许）→ 可选 NL → **`run_staged_plan_steps_loop`**。
    FullPipelineThenSteps,
}

#[inline]
pub(crate) fn prepared_post_parse_schedule(plan_no_task: bool) -> PreparedPostParseSchedule {
    if plan_no_task {
        PreparedPostParseSchedule::NoTaskThenOuter
    } else {
        PreparedPostParseSchedule::FullPipelineThenSteps
    }
}

/// **`FullPipelineThenSteps`** 下 ensemble → 优化 →（可选）NL → 分步循环 的一次性门控快照（纯函数构造，无 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreparedFullPipelineSchedule {
    pub(crate) ensemble_route: StagedPlanEnsembleRoute,
    pub(crate) optimizer_route: StagedPlanOptimizerRoute,
    /// 与 **`AgentConfig::staged_plan_two_phase_nl_display`** 一致：在进入 **`run_staged_plan_steps_loop`** 前是否多一轮 NL。
    pub(crate) nl_followup_before_steps: bool,
}

/// 构造 **`PreparedFullPipelineSchedule`** 所需的只读输入（生存期与调用方持有的 **`messages` / CSV 缓冲** 对齐即可）。
pub(crate) struct PreparedFullPipelineInputs<'a> {
    pub(crate) staged_plan_ensemble_count: u8,
    pub(crate) staged_plan_skip_ensemble_on_casual_prompt: bool,
    pub(crate) validate_only_binding_active: bool,
    pub(crate) trigger_user_content: Option<&'a str>,
    pub(crate) plan_steps_len: usize,
    pub(crate) staged_plan_optimizer_round: bool,
    pub(crate) staged_plan_optimizer_requires_parallel_tools: bool,
    pub(crate) parallel_tool_names_csv: &'a str,
    pub(crate) staged_plan_two_phase_nl_display: bool,
}

#[inline]
pub(crate) fn prepared_full_pipeline_schedule(
    inputs: PreparedFullPipelineInputs<'_>,
) -> PreparedFullPipelineSchedule {
    let ensemble_route = staged_plan_ensemble_route(
        inputs.staged_plan_ensemble_count,
        inputs.staged_plan_skip_ensemble_on_casual_prompt,
        inputs.validate_only_binding_active,
        inputs.trigger_user_content,
    );
    let optimizer_route = staged_plan_optimizer_route(
        inputs.plan_steps_len,
        inputs.staged_plan_optimizer_round,
        inputs.validate_only_binding_active,
        inputs.staged_plan_optimizer_requires_parallel_tools,
        inputs.parallel_tool_names_csv,
    );
    PreparedFullPipelineSchedule {
        ensemble_route,
        optimizer_route,
        nl_followup_before_steps: inputs.staged_plan_two_phase_nl_display,
    }
}

#[cfg(test)]
mod tests {
    use super::super::planner_round_fsm::{StagedPlanEnsembleRoute, StagedPlanOptimizerRoute};
    use super::*;

    #[test]
    fn no_task_branch() {
        assert_eq!(
            prepared_post_parse_schedule(true),
            PreparedPostParseSchedule::NoTaskThenOuter
        );
    }

    #[test]
    fn structured_branch() {
        assert_eq!(
            prepared_post_parse_schedule(false),
            PreparedPostParseSchedule::FullPipelineThenSteps
        );
    }

    #[test]
    fn full_pipeline_bundles_routes_and_nl_flag() {
        let s = prepared_full_pipeline_schedule(PreparedFullPipelineInputs {
            staged_plan_ensemble_count: 1,
            staged_plan_skip_ensemble_on_casual_prompt: false,
            validate_only_binding_active: false,
            trigger_user_content: Some("task"),
            plan_steps_len: 3,
            staged_plan_optimizer_round: true,
            staged_plan_optimizer_requires_parallel_tools: false,
            parallel_tool_names_csv: "a,b",
            staged_plan_two_phase_nl_display: true,
        });
        assert_eq!(s.ensemble_route, StagedPlanEnsembleRoute::SkipNotConfigured);
        assert_eq!(s.optimizer_route, StagedPlanOptimizerRoute::Run);
        assert!(s.nl_followup_before_steps);
    }

    #[test]
    fn full_pipeline_optimizer_skips_single_step_even_if_nl_enabled() {
        let s = prepared_full_pipeline_schedule(PreparedFullPipelineInputs {
            staged_plan_ensemble_count: 3,
            staged_plan_skip_ensemble_on_casual_prompt: false,
            validate_only_binding_active: false,
            trigger_user_content: Some("long task"),
            plan_steps_len: 1,
            staged_plan_optimizer_round: true,
            staged_plan_optimizer_requires_parallel_tools: false,
            parallel_tool_names_csv: "x",
            staged_plan_two_phase_nl_display: false,
        });
        assert_eq!(s.ensemble_route, StagedPlanEnsembleRoute::Run);
        assert_eq!(s.optimizer_route, StagedPlanOptimizerRoute::SkipStepsLt2);
        assert!(!s.nl_followup_before_steps);
    }
}
