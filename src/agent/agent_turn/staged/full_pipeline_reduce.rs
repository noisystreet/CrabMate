//! 首轮后 **full-pipeline** 各段（ensemble / 优化 / NL）→ 无 IO 的 reduce（表驱动；IO 仍在 **`mod.rs`**）。

use super::full_pipeline_fsm::StagedFullPipelinePhase;
use super::post_parse_pipeline_fsm::{ensemble_merge_should_invoke, optimizer_round_should_run};
use super::prepared_post_parse_fsm::PreparedFullPipelineSchedule;

/// full-pipeline 线性段（与 **`StagedFullPipelinePhase`** 推进顺序一致）。
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

/// 段级 reduce：是否应发起该段 LLM（跳过仍推进相位）。
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

/// 段完成后应记录的 **`StagedFullPipelinePhase`**（无论 Run/Skip）。
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
    use super::super::planner_round_fsm::{StagedPlanEnsembleRoute, StagedPlanOptimizerRoute};
    use super::*;

    fn schedule(
        ensemble: StagedPlanEnsembleRoute,
        optimizer: StagedPlanOptimizerRoute,
        nl: bool,
    ) -> PreparedFullPipelineSchedule {
        PreparedFullPipelineSchedule {
            ensemble_route: ensemble,
            optimizer_route: optimizer,
            nl_followup_before_steps: nl,
        }
    }

    #[test]
    fn segment_reduce_respects_routes() {
        let s = schedule(
            StagedPlanEnsembleRoute::SkipValidateOnlyBinding,
            StagedPlanOptimizerRoute::Run,
            true,
        );
        assert_eq!(
            reduce_full_pipeline_segment(FullPipelineSegment::Ensemble, &s),
            FullPipelineSegmentReduceAction::SkipSegment
        );
        assert_eq!(
            reduce_full_pipeline_segment(FullPipelineSegment::Optimizer, &s),
            FullPipelineSegmentReduceAction::RunLlm
        );
        assert_eq!(
            reduce_full_pipeline_segment(FullPipelineSegment::NlFollowup, &s),
            FullPipelineSegmentReduceAction::RunLlm
        );
    }

    #[test]
    fn phase_after_segment_linear() {
        assert_eq!(
            full_pipeline_phase_after_segment(FullPipelineSegment::Ensemble),
            StagedFullPipelinePhase::AfterEnsemble
        );
        assert_eq!(
            full_pipeline_phase_after_segment(FullPipelineSegment::NlFollowup),
            StagedFullPipelinePhase::AfterNlFollowup
        );
    }
}
