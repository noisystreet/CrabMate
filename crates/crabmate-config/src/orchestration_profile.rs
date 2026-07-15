//! 编排档位：映射现有门控键，不引入第四套实现。

/// 用户可见三档编排策略（与 `planner_executor_mode` / 分阶段门控正交）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationProfile {
    /// 非分层下强制走外循环 ReAct（推理-行动-观察）。
    ReAct,
    /// 非分层下对 Execute 类任务尽量走分阶段（覆盖 advisory/readonly bypass）。
    Staged,
    /// 现有 L2 + `staged_plan_intent_gate` 行为（默认）。
    #[default]
    Auto,
}

impl OrchestrationProfile {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "react" => Ok(Self::ReAct),
            "staged" => Ok(Self::Staged),
            "auto" => Ok(Self::Auto),
            _ => Err(format!(
                "未知 orchestration_profile {:?}，应为 react / staged / auto",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
            Self::Staged => "staged",
            Self::Auto => "auto",
        }
    }
}

/// 本进程有效编排路径摘要（`doctor` / `GET /status`；不含用户任务级门控结果）。
pub fn effective_orchestration_path_summary(
    planner_executor_mode: &str,
    profile: OrchestrationProfile,
) -> String {
    match planner_executor_mode {
        "hierarchical" => format!(
            "hierarchical (profile={}; intent_gate → Manager/Operator 或 discourse fallback)",
            profile.as_str()
        ),
        "logical_dual_agent" => match profile {
            OrchestrationProfile::ReAct => {
                "non_hierarchical: react outer loop (profile=react overrides staged gate)"
                    .to_string()
            }
            OrchestrationProfile::Staged => {
                "non_hierarchical: planned_step logical_dual (profile=staged prefers staged)"
                    .to_string()
            }
            OrchestrationProfile::Auto => {
                "non_hierarchical: staged_plan_intent_gate → planned_step logical_dual | react"
                    .to_string()
            }
        },
        _ => match profile {
            OrchestrationProfile::ReAct => {
                "non_hierarchical: react outer loop (profile=react overrides staged gate)"
                    .to_string()
            }
            OrchestrationProfile::Staged => {
                "non_hierarchical: staged_plan_intent_gate → planned_step single_agent (profile=staged; bypass overrides advisory/readonly deny)"
                    .to_string()
            }
            OrchestrationProfile::Auto => {
                "non_hierarchical: staged_plan_intent_gate → planned_step single_agent | react"
                    .to_string()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_variants() {
        assert_eq!(
            OrchestrationProfile::parse("auto").unwrap(),
            OrchestrationProfile::Auto
        );
        assert_eq!(
            OrchestrationProfile::parse("react").unwrap(),
            OrchestrationProfile::ReAct
        );
    }
}
