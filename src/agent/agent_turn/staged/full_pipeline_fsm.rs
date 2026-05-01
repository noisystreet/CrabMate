//! 首轮 **`agent_reply_plan` 解析成功**且走 **`PreparedPostParseSchedule::FullPipelineThenSteps`** 时，
//! **ensemble → 优化轮 → 可选 NL followup → `run_staged_plan_steps_loop`** 的**显式线性相位**（见 `docs/design/per_state_machine_consolidation.md` §3.2）。
//! **不**替代 `PreparedFullPipelineSchedule` 门控；仅把「执行到第几段」收成枚举 + 统一 `debug!`，便于观测与后续扩展非线性转移。

use log::debug;

/// 分步循环 **`run_staged_plan_steps_loop`** 之前的管线相位（严格线性顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedFullPipelinePhase {
    /// 尚未执行 / 跳过 ensemble 合并逻辑（入口即此相位）。
    BeforeEnsemble,
    /// ensemble 段已结束（运行或跳过），尚未进入优化轮段。
    AfterEnsemble,
    /// 优化轮段已结束（运行或跳过），尚未进入 NL followup。
    AfterOptimizer,
    /// NL followup 已结束（运行或跳过），尚未分配 `plan_id` / 进入分步循环。
    AfterNlFollowup,
}

impl StagedFullPipelinePhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::BeforeEnsemble => "before_ensemble",
            Self::AfterEnsemble => "after_ensemble",
            Self::AfterOptimizer => "after_optimizer",
            Self::AfterNlFollowup => "after_nl_followup",
        }
    }

    /// 线性推进；`AfterNlFollowup` 之后由驱动层进入分步循环（本枚举不表示步内状态）。
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
