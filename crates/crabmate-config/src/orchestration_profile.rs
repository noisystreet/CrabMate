//! 编排档位：仅 ReAct（推理-行动-观察）。

/// 编排策略——仅 ReAct（单 Agent 外循环）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationProfile {
    /// 非分层下强制走外循环 ReAct（推理-行动-观察）。
    #[default]
    ReAct,
}

impl OrchestrationProfile {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "react" => Ok(Self::ReAct),
            _ => Err(format!(
                "未知 orchestration_profile {:?}，应为 react",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
        }
    }
}

/// 本进程有效编排路径摘要（`doctor` / `GET /status`；不含用户任务级门控结果）。
pub fn effective_orchestration_path_summary(
    _planner_executor_mode: &str,
    _profile: OrchestrationProfile,
) -> String {
    "non_hierarchical: react outer loop".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_react() {
        assert_eq!(
            OrchestrationProfile::parse("react").unwrap(),
            OrchestrationProfile::ReAct
        );
    }

    #[test]
    fn parse_profile_rejects_unknown() {
        assert!(OrchestrationProfile::parse("staged").is_err());
        assert!(OrchestrationProfile::parse("auto").is_err());
    }

    #[test]
    fn effective_summary_is_react() {
        assert_eq!(
            effective_orchestration_path_summary("single_agent", OrchestrationProfile::ReAct),
            "non_hierarchical: react outer loop"
        );
    }
}
