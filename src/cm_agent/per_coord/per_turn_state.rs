//! 单轮 `run_agent_turn` 内与 PER 协调相关的**可变回合状态**，从 [`super::PerCoordinator`] 顶层字段拆出，
//! 便于一眼区分：**配置镜像 / 策略来源** vs **本回合计数** vs **派生缓存** vs **工具失败短路表**。
//!
//! 归属总表见 **`docs/design/run_loop_state_ownership.md`**。
//!
//! - **[`PerTurnCounters`]**：终答 `plan_rewrite` 已用次数 + 暂住的 **[`OuterLoopReflectMemo`]**（R 轨 3）。
//! - **[`WorkflowValidateLayerCache`]**：`last_workflow_validate_layer_count` 随 `messages.len()` 的缓存；上下文裁剪后必须失效。
//! - **[`RepeatedToolFailureMemo`]**：同轮工具失败签名 / 族短路（只读查询 + 记录清除）。
//! - **[`SuccessfulRunCommandDedupeMemo`]**：同轮已成功构建/运行命令的结果缓存（防重复 spawn）。

use crate::cm_types::Message;
use std::collections::HashMap;

use crate::cm_agent::plan_rewrite;

/// 外循环 Gate **前**纠偏计数（R 轨 3：`OuterLoopReflectPreGateReason`）。
///
/// **语义归属**：外循环 / `outer_loop_reflect`，**不是**终答 Gate 或工作流反思 FSM。
/// **物理位置**：暂住 [`PerTurnCounters`]（经 `PerCoordinator` 委托），便于单轮共享；勿与
/// `plan_rewrite_attempts` 混读。见 **`docs/design/run_loop_state_ownership.md`**。
#[derive(Debug, Clone, Default)]
pub(crate) struct OuterLoopReflectMemo {
    pub(crate) build_idle_streak: u32,
    pub(crate) build_idle_feedback_injected: u32,
    pub(crate) missing_final_answer_feedback_injected: u32,
}

impl OuterLoopReflectMemo {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_build_idle_round(&mut self) -> u32 {
        self.build_idle_streak = self.build_idle_streak.saturating_add(1);
        self.build_idle_streak
    }

    pub(crate) fn reset_build_idle_streak(&mut self) {
        self.build_idle_streak = 0;
    }

    pub(crate) fn record_build_idle_feedback_injected(&mut self) {
        self.build_idle_feedback_injected = self.build_idle_feedback_injected.saturating_add(1);
    }

    pub(crate) fn build_idle_feedback_injected(&self) -> u32 {
        self.build_idle_feedback_injected
    }

    pub(crate) fn record_missing_final_answer_feedback_injected(&mut self) {
        self.missing_final_answer_feedback_injected = self
            .missing_final_answer_feedback_injected
            .saturating_add(1);
    }

    pub(crate) fn missing_final_answer_feedback_injected(&self) -> u32 {
        self.missing_final_answer_feedback_injected
    }
}

/// 本 `run_agent_turn` 内、与配置上限对照的**正交**计数器。
///
/// - **`plan_rewrite_attempts`**：终答路径 `agent_reply_plan` 不合格时追加重写 user 的已用次数（与 **`plan_rewrite_max_attempts`** 对照）。
/// - **`outer_loop_reflect`**：外循环 pre-gate 纠偏（见 [`OuterLoopReflectMemo`]）。
#[derive(Debug, Clone)]
pub(crate) struct PerTurnCounters {
    pub(crate) plan_rewrite_attempts: usize,
    pub(crate) outer_loop_reflect: OuterLoopReflectMemo,
}

impl PerTurnCounters {
    pub(crate) fn new() -> Self {
        Self {
            plan_rewrite_attempts: 0,
            outer_loop_reflect: OuterLoopReflectMemo::new(),
        }
    }

    pub(crate) fn record_outer_loop_build_idle_round(&mut self) -> u32 {
        self.outer_loop_reflect.record_build_idle_round()
    }

    pub(crate) fn reset_outer_loop_build_idle_streak(&mut self) {
        self.outer_loop_reflect.reset_build_idle_streak();
    }

    pub(crate) fn record_outer_loop_build_idle_feedback_injected(&mut self) {
        self.outer_loop_reflect
            .record_build_idle_feedback_injected();
    }

    pub(crate) fn outer_loop_build_idle_feedback_injected(&self) -> u32 {
        self.outer_loop_reflect.build_idle_feedback_injected()
    }

    pub(crate) fn record_outer_loop_missing_final_answer_feedback_injected(&mut self) {
        self.outer_loop_reflect
            .record_missing_final_answer_feedback_injected();
    }

    pub(crate) fn outer_loop_missing_final_answer_feedback_injected(&self) -> u32 {
        self.outer_loop_reflect
            .missing_final_answer_feedback_injected()
    }
}

/// 缓存 [`plan_rewrite::last_workflow_validate_layer_count`]：仅在 `messages.len()` 与上次一致且已有缓存时跳过全表扫描。
#[derive(Debug, Clone)]
pub(crate) struct WorkflowValidateLayerCache {
    cached_workflow_validate_layer_count: Option<usize>,
    layer_count_cache_at_message_len: usize,
}

impl WorkflowValidateLayerCache {
    pub(crate) fn new() -> Self {
        Self {
            cached_workflow_validate_layer_count: None,
            layer_count_cache_at_message_len: 0,
        }
    }

    pub(crate) fn invalidate_after_context_mutation(&mut self) {
        self.cached_workflow_validate_layer_count = None;
        self.layer_count_cache_at_message_len = 0;
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> (Option<usize>, usize) {
        (
            self.cached_workflow_validate_layer_count,
            self.layer_count_cache_at_message_len,
        )
    }

    pub(crate) fn workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        let len = messages.len();
        if len != self.layer_count_cache_at_message_len {
            let n = plan_rewrite::last_workflow_validate_layer_count(messages);
            self.cached_workflow_validate_layer_count = n;
            self.layer_count_cache_at_message_len = len;
            return n;
        }
        if self.cached_workflow_validate_layer_count.is_some() {
            return self.cached_workflow_validate_layer_count;
        }
        let n = plan_rewrite::last_workflow_validate_layer_count(messages);
        self.cached_workflow_validate_layer_count = n;
        self.layer_count_cache_at_message_len = len;
        n
    }

    /// `append_tool_result_and_reflection` 在追加 tool（及可选 user）后同步缓存与扫描结果。
    pub(crate) fn refresh_after_messages_append(
        &mut self,
        messages_len: usize,
        messages: &[Message],
    ) {
        self.layer_count_cache_at_message_len = messages_len;
        self.cached_workflow_validate_layer_count =
            plan_rewrite::last_workflow_validate_layer_count(messages);
    }
}

/// 同一回合内工具失败记忆：精确签名与「错误族」两级短路。
#[derive(Debug, Clone)]
pub(crate) struct RepeatedToolFailureMemo {
    repeated_failed_tool_signatures: HashMap<(String, String), String>,
    repeated_failed_tool_families: HashMap<(String, String), String>,
}

impl RepeatedToolFailureMemo {
    pub(crate) fn new() -> Self {
        Self {
            repeated_failed_tool_signatures: HashMap::new(),
            repeated_failed_tool_families: HashMap::new(),
        }
    }

    pub(crate) fn repeated_tool_failure_error_marker(
        &self,
        tool_name: &str,
        tool_args_json: &str,
    ) -> Option<&str> {
        self.repeated_failed_tool_signatures
            .get(&(tool_name.to_string(), tool_args_json.to_string()))
            .map(|s| s.as_str())
    }

    pub(crate) fn mark_tool_failure_signature(
        &mut self,
        tool_name: &str,
        tool_args_json: &str,
        error_marker: String,
    ) {
        self.repeated_failed_tool_signatures.insert(
            (tool_name.to_string(), tool_args_json.to_string()),
            error_marker,
        );
    }

    pub(crate) fn repeated_tool_failure_family_marker(
        &self,
        tool_name: &str,
        failure_family: &str,
    ) -> Option<&str> {
        self.repeated_failed_tool_families
            .get(&(tool_name.to_string(), failure_family.to_string()))
            .map(|s| s.as_str())
    }

    pub(crate) fn mark_tool_failure_family(
        &mut self,
        tool_name: &str,
        failure_family: &str,
        error_marker: String,
    ) {
        self.repeated_failed_tool_families.insert(
            (tool_name.to_string(), failure_family.to_string()),
            error_marker,
        );
    }

    pub(crate) fn clear_tool_failure_signature(&mut self, tool_name: &str, tool_args_json: &str) {
        self.repeated_failed_tool_signatures
            .remove(&(tool_name.to_string(), tool_args_json.to_string()));
    }

    pub(crate) fn clear_tool_failure_families_for_tool(&mut self, tool_name: &str) {
        self.repeated_failed_tool_families
            .retain(|(name, _), _| name != tool_name);
    }

    pub(crate) fn clear_all_tool_failure_state_for_tool(&mut self, tool_name: &str) {
        self.repeated_failed_tool_signatures
            .retain(|(name, _), _| name != tool_name);
        self.clear_tool_failure_families_for_tool(tool_name);
    }
}

/// 同一回合内已成功执行过的构建/运行类 `run_command` 缓存。
#[derive(Debug, Clone, Default)]
pub(crate) struct SuccessfulRunCommandDedupeMemo {
    outputs: HashMap<String, String>,
}

impl SuccessfulRunCommandDedupeMemo {
    pub(crate) fn new() -> Self {
        Self {
            outputs: HashMap::new(),
        }
    }

    pub(crate) fn cached_output(&self, suppress_key: &str) -> Option<&str> {
        self.outputs.get(suppress_key).map(|s| s.as_str())
    }

    pub(crate) fn record_success(&mut self, suppress_key: String, output: String) {
        self.outputs.insert(suppress_key, output);
    }

    pub(crate) fn clear_all(&mut self) {
        self.outputs.clear();
    }
}
