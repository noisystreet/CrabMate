//! 规划–执行–反思（PER）协调：**工作流反思**状态机（`prepare_workflow_execute` / `append_tool_result_and_reflection`）与 **`PerCoordinator` 回合状态**（规划需求来源、**终答 `plan_rewrite` 计数**、**分阶段补丁规划累计轮次**（与前者独立）、`after_final_assistant` 分支）。
//! 可变回合侧字段按职责拆入 **[`per_turn_state`]**（计数 / `workflow_validate` 层缓存 / 工具失败短路表），减少顶层「一锅烩」。
//! 终答规划 JSON 的**静态校验**、重写 user 文案组装、历史里 `workflow_validate` 扫描与侧向校验**摘要**在 [`crate::plan_rewrite`]；侧向 **LLM** 调用仍在根包 `crabmate::agent::per_plan_semantic_check`。
//! Web 与 CLI 的 `run_agent_turn` 共用此层。
//!
//! 终答规划门控（`after_final_assistant` 决策树）见 [`final_plan_gate`]。

pub mod final_plan_gate;
mod final_plan_gate_context;
mod final_plan_gate_reason;
mod per_turn_state;

/// 何时要求模型在**最终** assistant 正文中嵌入可解析的 `agent_reply_plan` v1（见 `plan_artifact`）。
pub use crabmate_config::FinalPlanRequirementMode;

use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::plan_artifact;
use crate::plan_rewrite;
use crate::workflow_reflection_controller::{self, WorkflowReflectionController};
use serde_json::Value;

pub use plan_rewrite::PlanRewriteExhaustedReason;

pub(crate) const PLAN_REWRITE_EXHAUSTED_SSE: &str =
    "结构化规划仍未满足要求（已达最大重写次数），已结束本轮；请调整需求后重试。";

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
            reflection_default_max_rounds: cfg.per_plan_policy.reflection_default_max_rounds,
            final_plan_policy: cfg.per_plan_policy.final_plan_requirement,
            plan_rewrite_max_attempts: cfg.per_plan_policy.plan_rewrite_max_attempts,
            staged_plan_patch_max_attempts_config: 3,
            final_plan_require_strict_workflow_node_coverage: cfg
                .per_plan_policy
                .final_plan_require_strict_workflow_node_coverage,
            final_plan_semantic_check_enabled: cfg
                .per_plan_policy
                .final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: cfg
                .per_plan_policy
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
    /// 本回合**可变计数**（终答 `plan_rewrite` vs 分阶段补丁已成功合并轮次）；详见 [`per_turn_state::PerTurnCounters`]。
    pub(crate) counters: per_turn_state::PerTurnCounters,
    pub(crate) workflow_validate_cache: per_turn_state::WorkflowValidateLayerCache,
    pub(crate) repeated_tool_failures: per_turn_state::RepeatedToolFailureMemo,
    pub(crate) successful_run_commands: per_turn_state::SuccessfulRunCommandDedupeMemo,
}

mod coordinator_impl;

#[cfg(test)]
mod final_plan_gate_golden;

#[cfg(test)]
impl PerCoordinator {
    fn increment_plan_rewrite_attempts(&mut self) {
        self.counters.plan_rewrite_attempts += 1;
    }

    fn test_workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        self.workflow_validate_layer_need(messages)
    }

    fn test_layer_cache_snapshot(&self) -> (Option<usize>, usize) {
        self.workflow_validate_cache.snapshot()
    }
}

#[cfg(test)]
mod tests;
