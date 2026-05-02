//! 规划–执行–反思（PER）协调：**工作流反思**状态机（`prepare_workflow_execute` / `append_tool_result_and_reflection`）与 **`PerCoordinator` 回合状态**（规划需求来源、**终答 `plan_rewrite` 计数**、**分阶段补丁规划累计轮次**（与前者独立）、`after_final_assistant` 分支）。
//! 终答规划 JSON 的**静态校验**、重写 user 文案组装、历史里 `workflow_validate` 扫描与侧向校验**摘要**在 [`super::reflection::plan_rewrite`]；侧向 **LLM** 调用在 [`super::per_plan_semantic_check`]。
//! Web 与 CLI 的 `run_agent_turn` 共用此层。
//!
//! 终答规划门控（`after_final_assistant` 决策树）见 [`final_plan_gate`]。

pub(crate) mod final_plan_gate;

use crate::config::AgentConfig;
use crate::types::Message;
use std::collections::HashMap;

use super::plan_artifact;
use super::reflection::plan_rewrite;
use super::workflow_reflection_controller::{self, WorkflowReflectionController};
use serde_json::Value;

pub use plan_rewrite::PlanRewriteExhaustedReason;

pub(crate) const PLAN_REWRITE_EXHAUSTED_SSE: &str =
    "结构化规划仍未满足要求（已达最大重写次数），已结束本轮；请调整需求后重试。";

/// 何时要求模型在**最终** assistant 正文中嵌入可解析的 `agent_reply_plan` v1（见 `plan_artifact`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalPlanRequirementMode {
    /// 从不强制；工作流反思仍会注入指令，但不触发 `after_final_assistant` 的重写循环。
    Never,
    /// 默认：仅当本轮工具路径注入了 [`workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`] 时，对随后的终答校验。
    #[default]
    WorkflowReflection,
    /// 每次模型以非 `tool_calls` 结束时均校验（实验性，易增加额外模型轮次）。
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

/// 标识 plan 需求的来源，使工作流反思与终答反思的交互点可审计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanRequirementSource {
    /// 无需求
    None,
    /// 来自 `FinalPlanRequirementMode::Always` 配置
    ConfigAlways,
    /// 来自工作流反思第一轮注入的 `INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`
    WorkflowReflection,
}

/// 构造 `PerCoordinator` 的运行期参数（嵌入默认 + 热重载后由 `AgentConfig` 填充）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerCoordinatorInit {
    pub reflection_default_max_rounds: usize,
    pub final_plan_policy: FinalPlanRequirementMode,
    pub plan_rewrite_max_attempts: usize,
    /// 分阶段 **`patch_planner`** 路径下，配置项 **`staged_plan_patch_max_attempts`** 的镜像（仅用于审计/反馈文案；**不**改变 `plan_rewrite_max_attempts`）。
    pub staged_plan_patch_max_attempts_config: usize,
    /// 为 true 时：若任一步填写 `workflow_node_id`，则须覆盖最近一次工作流工具结果中的**全部** `nodes[].id`。
    pub final_plan_require_strict_workflow_node_coverage: bool,
    /// 可选二次 LLM：对比规划 JSON 与最近工具摘要；默认 false。
    pub final_plan_semantic_check_enabled: bool,
    /// 语义校验摘要中最多收录的**非只读**工具条数（0 表示不收录写类工具正文）。
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
}

impl PerCoordinatorInit {
    /// 与 [`AgentConfig`] 中 PER 协调相关字段对齐；供默认外循环与分层「话语型」回落路径共用，避免两处分叉漂移。
    pub fn from_agent_config(cfg: &AgentConfig) -> Self {
        Self {
            reflection_default_max_rounds: cfg.reflection_default_max_rounds,
            final_plan_policy: cfg.final_plan_requirement,
            plan_rewrite_max_attempts: cfg.plan_rewrite_max_attempts,
            staged_plan_patch_max_attempts_config: cfg.staged_plan_patch_max_attempts,
            final_plan_require_strict_workflow_node_coverage: cfg
                .final_plan_require_strict_workflow_node_coverage,
            final_plan_semantic_check_enabled: cfg.final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: cfg
                .final_plan_semantic_check_max_non_readonly_tools,
        }
    }
}

/// 模型返回最终文本（非 tool_calls）后，由协调层决定是结束本轮还是要求重写。
#[derive(Debug)]
pub enum AfterFinalAssistant {
    /// 结束 `run_agent_turn` 外层的本次循环
    StopTurn,
    /// 追加一条 user 消息并继续请求模型
    RequestPlanRewrite(Message),
    /// 已达 `plan_rewrite_max_attempts` 且规划仍不合格；assistant 已在 `messages` 中，由运行时发 SSE 后结束。
    StopTurnPlanRewriteExhausted { reason: PlanRewriteExhaustedReason },
    /// 静态规则已通过；需异步跑一次极短侧向 LLM 再决定结束或重写（不计入 `plan_rewrite_attempts` 直至判定不一致后追加重写）。
    StopTurnPendingPlanConsistencyLlm {
        plan: plan_artifact::AgentReplyPlanV1,
        tool_digest: Option<String>,
    },
}

/// `workflow_execute` 经反思控制器处理后的结果：要么执行补丁后的参数，要么直接返回跳过结果字符串。
#[derive(Debug)]
pub struct PreparedWorkflowExecute {
    pub patched_args: String,
    pub execute: bool,
    /// 当 `execute == false` 时作为 tool 结果内容
    pub skipped_result: String,
    pub reflection_inject: Option<Value>,
}

/// Web / CLI 共用的 PER 状态。
pub struct PerCoordinator {
    reflection: WorkflowReflectionController,
    final_plan_policy: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    /// 配置 **`staged_plan_patch_max_attempts`** 的镜像（供补丁反馈与 `/status` 展示；与 **`plan_rewrite_max_attempts`** 正交）。
    staged_plan_patch_max_attempts_config: usize,
    final_plan_require_strict_workflow_node_coverage: bool,
    final_plan_semantic_check_enabled: bool,
    final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 在 [`FinalPlanRequirementMode::WorkflowReflection`] 下，由 `prepare_workflow_execute` 根据反思注入置位。
    plan_requirement_source: PlanRequirementSource,
    plan_rewrite_attempts: usize,
    /// 本 `run_agent_turn` 回合内，**已成功完成**（解析并合并 `steps`）的**分阶段补丁规划**无工具轮次数。
    /// **不**计入终答路径的 **`plan_rewrite_attempts`**；与 **`staged_plan_patch_max_attempts`** 所限制的「单步失败分支内尝试次数」亦不同（后者为局部循环上界）。
    staged_plan_patch_planner_rounds_completed: usize,
    /// 缓存 [`last_workflow_validate_layer_count`]：`messages.len()` 未变时复用上一次的扫描结果。
    /// [`Self::append_tool_result_and_reflection`] 在追加后按新历史重算；[`Self::invalidate_workflow_validate_layer_cache_after_context_mutation`] 在上下文裁剪/摘要后清空，避免误用旧值。
    cached_workflow_validate_layer_count: Option<usize>,
    layer_count_cache_at_message_len: usize,
    /// 同一回合内已发生失败的工具签名：`(tool_name, tool_args_json) -> error_marker`。
    /// 用于“同命令同错误短路”，避免模型原样重试。
    repeated_failed_tool_signatures: HashMap<(String, String), String>,
    /// 同一回合内已发生失败的工具“错误族”：`(tool_name, failure_family) -> sample_error_marker`。
    /// 用于“同类失败短路”，避免仅改写命令形态却继续踩同一类约束。
    repeated_failed_tool_families: HashMap<(String, String), String>,
}

mod coordinator_impl;

#[cfg(test)]
impl PerCoordinator {
    fn increment_plan_rewrite_attempts(&mut self) {
        self.plan_rewrite_attempts += 1;
    }

    fn test_workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        self.workflow_validate_layer_need(messages)
    }

    fn test_layer_cache_snapshot(&self) -> (Option<usize>, usize) {
        (
            self.cached_workflow_validate_layer_count,
            self.layer_count_cache_at_message_len,
        )
    }
}

#[cfg(test)]
mod tests;
