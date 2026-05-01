//! **`resolve_parse_with_assistant`** 成功之后、步循环之前的 **post-parse 调度**（无 IO）：
//! **`no_task`** 与 **结构化 steps** 两条路径分叉，避免 `mod.rs` 中重复的 **`if plan.no_task`** 与后续 ensemble 块交织。
//!
//! 与 **`planner_round_fsm`** / **`post_parse_pipeline_fsm`**（ensemble/优化 **是否运行**）正交。

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

#[cfg(test)]
mod tests {
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
}
