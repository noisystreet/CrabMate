//! 首轮 **`agent_reply_plan` 解析成功之后**、步入执行循环之前的**规划管线调度**（纯函数，无 LLM）。
//!
//! 收拢原 **`planner_round_fsm`**、**`post_parse_pipeline_fsm`**、**`ensemble_fsm`**、
//! **`ensemble_schedule_fsm`**、**`full_pipeline_fsm`**、**`full_pipeline_reduce`**、
//! **`prepared_post_parse_fsm`**。
//!
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2（`PlanReady` 相位）。

use log::debug;

use crate::agent::plan_artifact::{AgentReplyPlanV1, PlanArtifactError, PlanStepV1};

// --- Post-parse schedule (原 prepared_post_parse_fsm) ---

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

// --- Ensemble / optimizer route (原 planner_round_fsm) ---

/// 逻辑多规划员（ensemble）是否在本轮执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanEnsembleRoute {
    SkipNotConfigured,
    SkipValidateOnlyBinding,
    SkipCasualHeuristic,
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
    PreparedFullPipelineSchedule {
        ensemble_route: staged_plan_ensemble_route(
            inputs.staged_plan_ensemble_count,
            inputs.staged_plan_skip_ensemble_on_casual_prompt,
            inputs.validate_only_binding_active,
            inputs.trigger_user_content,
        ),
        optimizer_route: staged_plan_optimizer_route(
            inputs.plan_steps_len,
            inputs.staged_plan_optimizer_round,
            inputs.validate_only_binding_active,
            inputs.staged_plan_optimizer_requires_parallel_tools,
            inputs.parallel_tool_names_csv,
        ),
        nl_followup_before_steps: inputs.staged_plan_two_phase_nl_display,
    }
}

// --- Post-parse invoke helpers (原 post_parse_pipeline_fsm) ---

#[inline]
pub(crate) fn ensemble_merge_should_invoke(route: StagedPlanEnsembleRoute) -> bool {
    !matches!(route, StagedPlanEnsembleRoute::SkipValidateOnlyBinding)
}

#[inline]
pub(crate) fn ensemble_merge_skip_for_casual_prompt(route: StagedPlanEnsembleRoute) -> bool {
    matches!(route, StagedPlanEnsembleRoute::SkipCasualHeuristic)
}

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

// --- Ensemble round decisions (原 ensemble_fsm) ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnsembleSecondaryPlannerRoundOutcome {
    AcceptAppend(AgentReplyPlanV1),
    StopChain,
}

pub(crate) fn ensemble_secondary_planner_round_outcome(
    parsed: Result<AgentReplyPlanV1, PlanArtifactError>,
) -> EnsembleSecondaryPlannerRoundOutcome {
    match parsed {
        Ok(p) if !p.no_task && !p.steps.is_empty() => {
            EnsembleSecondaryPlannerRoundOutcome::AcceptAppend(p)
        }
        Ok(_) | Err(_) => EnsembleSecondaryPlannerRoundOutcome::StopChain,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnsembleMergeOutcome {
    AppliedSteps(Vec<PlanStepV1>),
    KeepPriorPlan,
}

pub(crate) fn ensemble_merge_outcome_from_parsed_steps(
    merged_steps: Option<Vec<PlanStepV1>>,
) -> EnsembleMergeOutcome {
    match merged_steps {
        Some(steps) if !steps.is_empty() => EnsembleMergeOutcome::AppliedSteps(steps),
        Some(_) | None => EnsembleMergeOutcome::KeepPriorPlan,
    }
}

// --- Ensemble driver schedule (原 ensemble_schedule_fsm) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnsembleDriverPhase {
    Done,
    SecondaryChain { extra: u8 },
}

#[inline]
pub(crate) fn resolve_ensemble_driver_phase(
    staged_plan_ensemble_count: u8,
    skip_for_casual_user_prompt: bool,
) -> EnsembleDriverPhase {
    let extra = staged_plan_ensemble_count.saturating_sub(1);
    if extra == 0 || skip_for_casual_user_prompt {
        EnsembleDriverPhase::Done
    } else {
        EnsembleDriverPhase::SecondaryChain { extra }
    }
}

#[inline]
pub(crate) fn ensemble_secondary_planner_display_index(chain_round_index: u8) -> u8 {
    chain_round_index.saturating_add(2)
}

#[inline]
pub(crate) fn ensemble_merge_should_run(accepted_plans_len: usize) -> bool {
    accepted_plans_len >= 2
}

// --- Full pipeline linear phase (原 full_pipeline_fsm) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedFullPipelinePhase {
    BeforeEnsemble,
    AfterEnsemble,
    AfterOptimizer,
    AfterNlFollowup,
}

crate::impl_as_str!(StagedFullPipelinePhase, {
    Self::BeforeEnsemble => "before_ensemble",
    Self::AfterEnsemble => "after_ensemble",
    Self::AfterOptimizer => "after_optimizer",
    Self::AfterNlFollowup => "after_nl_followup",
});

impl StagedFullPipelinePhase {
    pub(crate) fn advance(self) -> Option<Self> {
        match self {
            Self::BeforeEnsemble => Some(Self::AfterEnsemble),
            Self::AfterEnsemble => Some(Self::AfterOptimizer),
            Self::AfterOptimizer => Some(Self::AfterNlFollowup),
            Self::AfterNlFollowup => None,
        }
    }
}

pub(crate) fn debug_staged_full_pipeline_enter(phase: StagedFullPipelinePhase) {
    debug!(
        target: "crabmate",
        "分阶段编排·首轮后管线：进入相位 (staged_fsm=full_pipeline phase={})",
        phase.as_str(),
    );
}

pub(crate) fn debug_staged_full_pipeline_transition(
    from: StagedFullPipelinePhase,
    to: Option<StagedFullPipelinePhase>,
) {
    match to {
        Some(next) => {
            debug!(
                target: "crabmate",
                "分阶段编排·首轮后管线：转移 (staged_fsm=full_pipeline from={} to={})",
                from.as_str(),
                next.as_str(),
            );
        }
        None => {
            debug!(
                target: "crabmate",
                "分阶段编排·首轮后管线：相位序列结束，进入分步执行循环 (staged_fsm=full_pipeline from={})",
                from.as_str(),
            );
        }
    }
}

// --- Full pipeline segment reduce (原 full_pipeline_reduce) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullPipelineSegment {
    Ensemble,
    Optimizer,
    NlFollowup,
}

impl FullPipelineSegment {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ensemble => "ensemble",
            Self::Optimizer => "optimizer",
            Self::NlFollowup => "nl_followup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullPipelineSegmentReduceAction {
    RunLlm,
    SkipSegment,
}

impl FullPipelineSegmentReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RunLlm => "run_llm",
            Self::SkipSegment => "skip_segment",
        }
    }
}

pub(crate) fn full_pipeline_entry_phase() -> StagedFullPipelinePhase {
    StagedFullPipelinePhase::BeforeEnsemble
}

pub(crate) fn full_pipeline_phase_after_segment(
    segment: FullPipelineSegment,
) -> StagedFullPipelinePhase {
    let prior = match segment {
        FullPipelineSegment::Ensemble => StagedFullPipelinePhase::BeforeEnsemble,
        FullPipelineSegment::Optimizer => StagedFullPipelinePhase::AfterEnsemble,
        FullPipelineSegment::NlFollowup => StagedFullPipelinePhase::AfterOptimizer,
    };
    prior.advance().unwrap_or_else(|| {
        tracing::error!(
            target: "crabmate::staged",
            segment = segment.as_str(),
            "full_pipeline: invalid segment advance; falling back to AfterNlFollowup"
        );
        StagedFullPipelinePhase::AfterNlFollowup
    })
}

pub(crate) fn reduce_full_pipeline_segment(
    segment: FullPipelineSegment,
    schedule: &PreparedFullPipelineSchedule,
) -> FullPipelineSegmentReduceAction {
    match segment {
        FullPipelineSegment::Ensemble => {
            if ensemble_merge_should_invoke(schedule.ensemble_route) {
                FullPipelineSegmentReduceAction::RunLlm
            } else {
                FullPipelineSegmentReduceAction::SkipSegment
            }
        }
        FullPipelineSegment::Optimizer => {
            if optimizer_round_should_run(schedule.optimizer_route) {
                FullPipelineSegmentReduceAction::RunLlm
            } else {
                FullPipelineSegmentReduceAction::SkipSegment
            }
        }
        FullPipelineSegment::NlFollowup => {
            if schedule.nl_followup_before_steps {
                FullPipelineSegmentReduceAction::RunLlm
            } else {
                FullPipelineSegmentReduceAction::SkipSegment
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_step(id: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: "d".to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    fn sample_plan(no_task: bool, steps: Vec<PlanStepV1>) -> AgentReplyPlanV1 {
        AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps,
            no_task,
        }
    }

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

    #[test]
    fn ensemble_invoke_skips_only_validate_binding() {
        assert!(!ensemble_merge_should_invoke(
            StagedPlanEnsembleRoute::SkipValidateOnlyBinding
        ));
        assert!(ensemble_merge_should_invoke(StagedPlanEnsembleRoute::Run));
    }

    #[test]
    fn optimizer_run_only_when_run_variant() {
        assert!(optimizer_round_should_run(StagedPlanOptimizerRoute::Run));
        assert!(!optimizer_round_should_run(
            StagedPlanOptimizerRoute::SkipStepsLt2
        ));
    }

    #[test]
    fn secondary_accepts_nonempty_non_no_task() {
        let p = sample_plan(false, vec![sample_step("s1")]);
        match ensemble_secondary_planner_round_outcome(Ok(p.clone())) {
            EnsembleSecondaryPlannerRoundOutcome::AcceptAppend(got) => assert_eq!(got, p),
            EnsembleSecondaryPlannerRoundOutcome::StopChain => panic!("expected AcceptAppend"),
        }
    }

    #[test]
    fn secondary_stops_on_parse_err() {
        assert!(matches!(
            ensemble_secondary_planner_round_outcome(Err(PlanArtifactError::NotFound)),
            EnsembleSecondaryPlannerRoundOutcome::StopChain
        ));
    }

    #[test]
    fn merge_applies_nonempty_steps() {
        let steps = vec![sample_step("a")];
        match ensemble_merge_outcome_from_parsed_steps(Some(steps.clone())) {
            EnsembleMergeOutcome::AppliedSteps(s) => assert_eq!(s, steps),
            EnsembleMergeOutcome::KeepPriorPlan => panic!("expected AppliedSteps"),
        }
    }

    #[test]
    fn chain_when_extra_positive_and_not_skipped() {
        match resolve_ensemble_driver_phase(3, false) {
            EnsembleDriverPhase::SecondaryChain { extra } => assert_eq!(extra, 2),
            EnsembleDriverPhase::Done => panic!("expected SecondaryChain"),
        }
    }

    #[test]
    fn merge_only_with_two_or_more_plans() {
        assert!(!ensemble_merge_should_run(1));
        assert!(ensemble_merge_should_run(2));
    }

    #[test]
    fn linear_advance_four_steps_then_terminal() {
        let mut p = StagedFullPipelinePhase::BeforeEnsemble;
        for expected in [
            StagedFullPipelinePhase::AfterEnsemble,
            StagedFullPipelinePhase::AfterOptimizer,
            StagedFullPipelinePhase::AfterNlFollowup,
        ] {
            let n = p.advance().expect("advance");
            assert_eq!(n, expected);
            p = n;
        }
        assert!(p.advance().is_none());
    }

    #[test]
    fn segment_reduce_respects_routes() {
        let s = PreparedFullPipelineSchedule {
            ensemble_route: StagedPlanEnsembleRoute::SkipValidateOnlyBinding,
            optimizer_route: StagedPlanOptimizerRoute::Run,
            nl_followup_before_steps: true,
        };
        assert_eq!(
            reduce_full_pipeline_segment(FullPipelineSegment::Ensemble, &s),
            FullPipelineSegmentReduceAction::SkipSegment
        );
        assert_eq!(
            reduce_full_pipeline_segment(FullPipelineSegment::Optimizer, &s),
            FullPipelineSegmentReduceAction::RunLlm
        );
    }

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
    fn full_pipeline_optimizer_skips_single_step_even_if_nl_disabled() {
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

    #[test]
    fn full_pipeline_bundles_via_inputs() {
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
    }
}
