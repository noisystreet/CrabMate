//! 编排档位：当前始终为 ReAct（单 Agent 外循环），不再提供 staging 选项。

/// 编排策略（当前仅 ReAct 一种有效值）。
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
        "react"
    }
}

/// 本进程有效编排路径摘要（`doctor` / `GET /status`）。
pub fn effective_orchestration_path_summary(
    _planner_executor_mode: &str,
    _profile: OrchestrationProfile,
) -> String {
    // 运行时只有 ReAct 外循环（`planner_executor_mode` 仅允许 single_agent）。
    "non_hierarchical: react outer loop".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_variants() {
        assert_eq!(
            OrchestrationProfile::parse("react").unwrap(),
            OrchestrationProfile::ReAct
        );
        assert!(OrchestrationProfile::parse("staged").is_err());
        assert!(OrchestrationProfile::parse("auto").is_err());
    }
}
