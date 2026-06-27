//! 工具批执行领域类型与模式判定；实际 `dispatch_tool` / workflow 经 [`ToolExecutionHost`] 注入。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use crabmate_config::AgentConfig;
use crabmate_internal::agent_role_turn::{
    tool_allowed_for_turn, tool_calls_allow_parallel_for_role, turn_tool_denied_message,
};
use crabmate_internal::tool_registry::{
    CliToolRuntime, DispatchToolParams, HandlerLookupTable, WebToolRuntime,
};
use crabmate_types::{Tool, ToolCall};

use crate::plan_artifact::PlanStepExecutorKind;
use crate::step_executor_policy::{
    executor_kind_tool_denied_body, tool_allowed_for_step_executor_kind,
};

/// 一批 tool 调用结束后的外层循环语义（与根包 `execute_tools` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

/// 只读并行批 vs 串行执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolBatchExecutionMode {
    ParallelReadonlyBatch,
    Serial,
}

/// [`resolve_tool_batch_execution_mode`] 入参。
pub struct ToolBatchModeParams<'a> {
    pub force_serial: bool,
    pub workspace_is_set: bool,
    pub handler_lookup: &'a HandlerLookupTable,
    pub cfg: &'a AgentConfig,
    pub tool_calls: &'a [ToolCall],
    pub turn_allow: Option<&'a HashSet<String>>,
}

/// `CM_REPLAY_FORCE_SERIAL` 是否为真值（与根包 replay 语义一致）。
pub fn replay_force_serial_from_env() -> bool {
    std::env::var("CM_REPLAY_FORCE_SERIAL")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

/// 并行只读批预取审批失败映射的键（与根包 `parallel_readonly` 对齐）。
pub type ParallelPrefetchFailureKey = (String, String);

/// 解析本批应采用并行只读还是串行。
pub fn resolve_tool_batch_execution_mode(
    params: &ToolBatchModeParams<'_>,
) -> ToolBatchExecutionMode {
    if params.force_serial || !params.workspace_is_set {
        return ToolBatchExecutionMode::Serial;
    }
    if tool_calls_allow_parallel_for_role(
        params.handler_lookup,
        params.cfg,
        params.tool_calls,
        params.turn_allow,
    ) {
        ToolBatchExecutionMode::ParallelReadonlyBatch
    } else {
        ToolBatchExecutionMode::Serial
    }
}

/// 统计并行只读批次中去重后的唯一 `(name, args)` 数。
pub fn dedup_readonly_tool_calls_count(tool_calls: &[ToolCall]) -> usize {
    let mut seen: HashSet<(&str, &str)> = HashSet::with_capacity(tool_calls.len());
    for tc in tool_calls {
        seen.insert((tc.function.name.as_str(), tc.function.arguments.as_str()));
    }
    seen.len()
}

/// 并行只读批预取审批失败（`name+args` → 错误正文）；由宿主在批前填充。
pub type ParallelPrefetchFailures = HashMap<ParallelPrefetchFailureKey, String>;

/// [`ToolExecutionHost::prefetch_parallel_approval_failures`] 入参。
pub struct ParallelPrefetchParams<'a> {
    pub tool_calls: &'a [ToolCall],
    pub cfg: &'a Arc<AgentConfig>,
    pub web_tool_ctx: Option<&'a WebToolRuntime>,
    pub cli_tool_ctx: Option<&'a CliToolRuntime>,
    pub handler_lookup: &'a HandlerLookupTable,
}

/// 子代理角色 / 多角色白名单的同步 early-deny 正文；`None` 表示可继续 dispatch。
pub struct ToolPolicyEarlyDenyParams<'a> {
    pub cfg: &'a AgentConfig,
    pub name: &'a str,
    pub step_executor_constraint: Option<PlanStepExecutorKind>,
    pub tools_defs: &'a [Tool],
    pub turn_allow: Option<&'a HashSet<String>>,
}

/// 串行 / 并行路径共用的策略 early-deny（不含 TTL / run_command 预检）。
pub fn tool_policy_early_deny_message(p: &ToolPolicyEarlyDenyParams<'_>) -> Option<String> {
    if let Some(k) = p.step_executor_constraint
        && !tool_allowed_for_step_executor_kind(p.cfg, p.name, k)
    {
        return Some(executor_kind_tool_denied_body(
            p.cfg,
            p.tools_defs,
            p.name,
            k,
        ));
    }
    if !tool_allowed_for_turn(p.name, p.turn_allow) {
        return Some(turn_tool_denied_message(p.name));
    }
    None
}

/// 根包实现的工具分发（`tool_registry::dispatch_tool` 与 `workflow_execute` 等）。
#[async_trait]
pub trait ToolExecutionHost: Send + Sync {
    async fn dispatch_tool_call(
        &mut self,
        name: &str,
        p: DispatchToolParams<'_>,
    ) -> (String, Option<serde_json::Value>);

    async fn prefetch_parallel_approval_failures(
        &self,
        params: ParallelPrefetchParams<'_>,
    ) -> ParallelPrefetchFailures;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_config::load_config;
    use crabmate_types::{FunctionCall, ToolCall};

    fn test_cfg() -> AgentConfig {
        load_config(None).expect("embed default")
    }

    fn tc(name: &str, args: &str, id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn dedup_counts_unique_name_args_pairs() {
        let calls = vec![
            tc("read_file", r#"{"path":"a"}"#, "1"),
            tc("read_file", r#"{"path":"a"}"#, "2"),
            tc("read_file", r#"{"path":"b"}"#, "3"),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 2);
        assert_eq!(dedup_readonly_tool_calls_count(&[]), 0);
    }

    #[test]
    fn force_serial_always_serial_mode() {
        let cfg = test_cfg();
        let lookup = HandlerLookupTable::default_dispatch();
        let calls = vec![tc("read_file", r#"{"path":"a"}"#, "1")];
        let mode = resolve_tool_batch_execution_mode(&ToolBatchModeParams {
            force_serial: true,
            workspace_is_set: true,
            handler_lookup: &lookup,
            cfg: &cfg,
            tool_calls: &calls,
            turn_allow: None,
        });
        assert_eq!(mode, ToolBatchExecutionMode::Serial);
    }

    #[test]
    fn workspace_unset_forces_serial() {
        let cfg = test_cfg();
        let lookup = HandlerLookupTable::default_dispatch();
        let calls = vec![tc("read_file", r#"{"path":"a"}"#, "1")];
        let mode = resolve_tool_batch_execution_mode(&ToolBatchModeParams {
            force_serial: false,
            workspace_is_set: false,
            handler_lookup: &lookup,
            cfg: &cfg,
            tool_calls: &calls,
            turn_allow: None,
        });
        assert_eq!(mode, ToolBatchExecutionMode::Serial);
    }
}
