//! `PlanSemanticLlmOutcome`：侧向语义 LLM 的解析结果。

/// 侧向语义校验的解析结果（供重写 user 消息附带 `violation_codes`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSemanticLlmOutcome {
    pub consistent: bool,
    /// 仅在 `consistent == false` 时有意义；经规范化，至少含一个后备码。
    pub violation_codes: Vec<String>,
    pub rationale: Option<String>,
    /// 侧向 LLM 调用被用户取消（须由编排层映射为 `TurnAborted`，不可 fail-open 为一致）。
    pub user_cancelled: bool,
}

impl PlanSemanticLlmOutcome {
    pub fn consistent_ok() -> Self {
        Self {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
            user_cancelled: false,
        }
    }

    pub fn user_cancelled() -> Self {
        Self {
            consistent: false,
            violation_codes: Vec::new(),
            rationale: None,
            user_cancelled: true,
        }
    }
}
