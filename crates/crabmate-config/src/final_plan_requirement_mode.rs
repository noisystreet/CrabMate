//! 终答规划 JSON 强制模式（配置项 `final_plan_requirement`）。

/// 何时要求模型在**最终** assistant 正文中嵌入可解析的 `agent_reply_plan` v1。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalPlanRequirementMode {
    /// 从不强制。
    Never,
    /// 默认：仅当本轮工具路径注入了工作流反思指令时，对随后的终答校验。
    #[default]
    WorkflowReflection,
    /// 每次模型以非 `tool_calls` 结束时均校验（实验性）。
    Always,
}

impl FinalPlanRequirementMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "never" => Ok(Self::Never),
            "workflow_reflection" => Ok(Self::WorkflowReflection),
            "always" => Ok(Self::Always),
            _ => Err(format!(
                "未知 final_plan_requirement {:?}，应为 never / workflow_reflection / always",
                s.trim()
            )),
        }
    }
}
