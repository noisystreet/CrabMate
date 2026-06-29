//! 单轮 `run_agent_turn` 内与 PER 协调相关的**可变回合状态**，从 [`super::PerCoordinator`] 顶层字段拆出，
//! 便于一眼区分：**配置镜像 / 策略来源** vs **本回合计数** vs **派生缓存** vs **工具失败短路表**。
//!
//! - **[`PerTurnCounters`]**：终答 `plan_rewrite` 与分阶段补丁规划「已成功合并轮次」两套**独立**计数（见模块级注释不变量）。
//! - **[`WorkflowValidateLayerCache`]**：`last_workflow_validate_layer_count` 随 `messages.len()` 的缓存；上下文裁剪后必须失效。
//! - **[`RepeatedToolFailureMemo`]**：同轮工具失败签名 / 族短路（只读查询 + 记录清除）。

use crate::types::Message;
use std::collections::HashMap;

use crate::agent::reflection::plan_rewrite;

/// 本 `run_agent_turn` 内、与配置上限对照的两套**正交**计数器。
///
/// - **`plan_rewrite_attempts`**：终答路径 `agent_reply_plan` 不合格时追加重写 user 的已用次数（与 **`plan_rewrite_max_attempts`** 对照）。
/// - **`staged_plan_patch_planner_rounds_completed`**：分阶段 **`patch_planner`** 路径下，已成功解析并合并 `steps` 的无工具轮次数（与 **`staged_plan_patch_max_attempts`** 约束的「单步失败分支内尝试」不同）。
/// - **`outer_loop_build_idle_streak`**：L2 外循环连续「承诺构建但无 tool_calls」轮次（见 **`outer_loop_build_idle`**）。
/// - **`outer_loop_build_idle_feedback_injected`**：已注入的构建空转纠偏 user 条数上限计数。
#[derive(Debug, Clone)]
pub(crate) struct PerTurnCounters {
    pub(crate) plan_rewrite_attempts: usize,
    pub(crate) staged_plan_patch_planner_rounds_completed: usize,
    pub(crate) outer_loop_build_idle_streak: u32,
    pub(crate) outer_loop_build_idle_feedback_injected: u32,
}

impl PerTurnCounters {
    pub(crate) fn new() -> Self {
        Self {
            plan_rewrite_attempts: 0,
            staged_plan_patch_planner_rounds_completed: 0,
            outer_loop_build_idle_streak: 0,
            outer_loop_build_idle_feedback_injected: 0,
        }
    }

    pub(crate) fn record_staged_plan_patch_planner_round_completed(&mut self) {
        self.staged_plan_patch_planner_rounds_completed = self
            .staged_plan_patch_planner_rounds_completed
            .saturating_add(1);
    }

    pub(crate) fn record_outer_loop_build_idle_round(&mut self) -> u32 {
        self.outer_loop_build_idle_streak = self.outer_loop_build_idle_streak.saturating_add(1);
        self.outer_loop_build_idle_streak
    }

    pub(crate) fn reset_outer_loop_build_idle_streak(&mut self) {
        self.outer_loop_build_idle_streak = 0;
    }

    pub(crate) fn record_outer_loop_build_idle_feedback_injected(&mut self) {
        self.outer_loop_build_idle_feedback_injected = self
            .outer_loop_build_idle_feedback_injected
            .saturating_add(1);
    }

    pub(crate) fn outer_loop_build_idle_feedback_injected(&self) -> u32 {
        self.outer_loop_build_idle_feedback_injected
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

#[cfg(test)]
mod per_turn_state_tests {
    use super::PerTurnCounters;

    #[test]
    fn staged_patch_round_counter_independent_of_plan_rewrite() {
        let mut c = PerTurnCounters::new();
        assert_eq!(c.plan_rewrite_attempts, 0);
        assert_eq!(c.staged_plan_patch_planner_rounds_completed, 0);
        c.record_staged_plan_patch_planner_round_completed();
        assert_eq!(c.plan_rewrite_attempts, 0);
        assert_eq!(c.staged_plan_patch_planner_rounds_completed, 1);
        c.plan_rewrite_attempts += 1;
        assert_eq!(c.plan_rewrite_attempts, 1);
        assert_eq!(c.staged_plan_patch_planner_rounds_completed, 1);
    }
}
