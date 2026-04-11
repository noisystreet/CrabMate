//! 规划重写控制器：封装 after_final_assistant 的规划重写策略与状态管理。
//!
//! 从 [`PerCoordinator`] 中迁出，专职负责：
//! - `FinalPlanRequirementMode` / `PlanRequirementSource` 状态机；
//! - `after_final_assistant` 中两种 `RequestPlanRewrite` 变体的构造；
//! - `StopTurnPendingPlanConsistencyLlm` 侧向 LLM 回调后的重写路由；
//! - 语义校验开关与违规反馈消息。

use crate::agent::reflection::plan_rewrite;
use crate::types::Message;

/// 常量前缀（已在 `per_coord.rs` 中定义，保持引用路径一致）。
const PLAN_REWRITE_EXHAUSTED_SSE: &str = super::per_coord::PLAN_REWRITE_EXHAUSTED_SSE;

/// 终答后规划重写策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalPlanRequirementMode {
    /// 不强制要求（仍可在 workflow 反思路径下由 `prepare_workflow_execute` 动态置位）。
    None,
    /// 每次终答（非 tool_calls）都必须包含 `agent_reply_plan`。
    Always,
    /// 仅在 workflow 反思路径下要求终答含 `agent_reply_plan`。
    WorkflowReflection,
}

/// 标记「当前是否必须包含可解析规划」的来源，用于日志与 SSE 诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRequirementSource {
    None,
    ConfigAlways,
    WorkflowReflection,
}

/// 迁入 [`PerCoordinatorInit`] 中规划重写相关的配置。
#[derive(Debug, Clone)]
pub struct PlanRewriteControllerInit {
    pub final_plan_policy: FinalPlanRequirementMode,
    pub plan_rewrite_max_attempts: usize,
    pub final_plan_semantic_check_enabled: bool,
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
}

/// 规划重写控制器。
#[derive(Debug, Clone)]
pub struct PlanRewriteController {
    final_plan_policy: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    final_plan_semantic_check_enabled: bool,
    final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 标记当前是否必须包含可解析规划（由 `ConfigAlways` 初始化，或由 workflow 反思路径动态置位）。
    plan_requirement_source: PlanRequirementSource,
    plan_rewrite_attempts: usize,
}

impl PlanRewriteController {
    pub fn new(init: PlanRewriteControllerInit) -> Self {
        let initial_source = match init.final_plan_policy {
            FinalPlanRequirementMode::Always => PlanRequirementSource::ConfigAlways,
            _ => PlanRequirementSource::None,
        };
        Self {
            final_plan_policy: init.final_plan_policy,
            plan_rewrite_max_attempts: init.plan_rewrite_max_attempts.max(1),
            final_plan_semantic_check_enabled: init.final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: init
                .final_plan_semantic_check_max_non_readonly_tools,
            plan_requirement_source: initial_source,
            plan_rewrite_attempts: 0,
        }
    }

    /// SSE `reason_code` 用尽的固定消息。
    pub fn exhausted_sse_message(&self) -> &'static str {
        PLAN_REWRITE_EXHAUSTED_SSE
    }

    /// 供 `/status` 等只读镜像：`after_final_assistant` 已递增后的重写次数。
    pub fn attempts_snapshot(&self) -> usize {
        self.plan_rewrite_attempts
    }

    /// 配置中的规划重写上限。
    pub fn max_attempts_limit(&self) -> usize {
        self.plan_rewrite_max_attempts
    }

    /// 侧向语义校验判定不一致后，递增重写计数（与 `RequestPlanRewrite` 路径一致）。
    #[allow(dead_code)]
    pub(crate) fn increment_attempts(&mut self) {
        self.plan_rewrite_attempts += 1;
    }

    /// 构建语义违规反馈 user 消息。
    pub fn semantic_mismatch_message(
        &self,
        violation_codes: &[String],
        rationale: Option<&str>,
    ) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(
                plan_rewrite::user_text_semantic_mismatch_with_feedback(violation_codes, rationale)
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 下一回模型若以非 `tool_calls` 结束，是否必须嵌入可解析的 `agent_reply_plan`。
    pub fn require_plan_flag_snapshot(&self) -> bool {
        self.plan_requirement_source != PlanRequirementSource::None
    }

    /// 当 workflow 反思控制器注入 `instruction_type` 为
    /// `INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT` 时调用，动态置位 plan_requirement_source。
    pub fn set_requirement_from_workflow_reflection(&mut self) {
        if self.final_plan_policy == FinalPlanRequirementMode::WorkflowReflection {
            self.plan_requirement_source = PlanRequirementSource::WorkflowReflection;
        }
    }

    /// 当前是否满足「需要终答含规划」的任一条件。
    pub fn is_plan_required(&self) -> bool {
        self.plan_requirement_source != PlanRequirementSource::None
    }

    /// 语义校验是否启用。
    pub fn semantic_check_enabled(&self) -> bool {
        self.final_plan_semantic_check_enabled
    }

    /// 语义校验允许的最大非只读工具数。
    pub fn semantic_check_max_non_readonly(&self) -> usize {
        self.final_plan_semantic_check_max_non_readonly_tools
    }

    /// 重新设置重试计数（在 workflow 切换或上下文重建后调用）。
    pub fn reset_attempts(&mut self) {
        self.plan_rewrite_attempts = 0;
    }

    /// 静态常量：SSE `reason_code` 置为 `plan_rewrite_exhausted`。
    pub const PLAN_REWRITE_EXHAUSTED_SSE: &'static str = PLAN_REWRITE_EXHAUSTED_SSE;
}
