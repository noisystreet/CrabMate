//! 编排档位：当前统一强制走 ReAct（单 Agent 外循环）。

/// 编排策略（当前仅 ReAct；`Staged` / `Auto` 已移除）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationProfile {
    /// 强制走外循环 ReAct（推理-行动-观察）。
    #[default]
    ReAct,
}

impl OrchestrationProfile {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "react" => Ok(Self::ReAct),
            _ => Err(format!(
                "未知 orchestration_profile {:?}，当前仅支持 react",
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
    planner_executor_mode: &str,
    _profile: OrchestrationProfile,
) -> String {
    match planner_executor_mode {
        "hierarchical" => {
            "hierarchical (profile=react; intent_gate → Manager/Operator 或 discourse fallback)"
                .to_string()
        }
        _ => "non_hierarchical: react outer loop (profile=react)".to_string(),
    }
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
        assert!(OrchestrationProfile::parse("auto").is_err());
    }
}
