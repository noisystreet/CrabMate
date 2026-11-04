//! 工具批执行领域类型与模式判定；实际 `dispatch_tool` / workflow 经根包 `ToolExecutionHost` 注入。

use std::collections::{HashMap, HashSet};

use crate::cm_config::AgentConfig;
use crate::cm_tools::tool_dispatch::HandlerLookupTable;
use crate::cm_types::ToolCall;

use crate::cm_agent::plan_artifact::PlanStepExecutorKind;
use crate::cm_agent::step_executor_policy::{
    executor_kind_tool_denied_body, tool_allowed_for_step_executor_kind,
};
use crate::agent_role_turn::{
    tool_allowed_for_turn, tool_calls_allow_parallel_for_role, turn_tool_denied_message,
};

/// 一批 tool 调用结束后的外层循环语义（与根包 `execute_tools` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteToolsBatchOutcome {
    Finished,
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

/// 子代理角色 / 多角色白名单 / 权限等级的同步 early-deny 正文；`None` 表示可继续 dispatch。
pub struct ToolPolicyEarlyDenyParams<'a> {
    pub cfg: &'a AgentConfig,
    pub name: &'a str,
    pub step_executor_constraint: Option<PlanStepExecutorKind>,
    pub tools_defs: &'a [crate::cm_types::Tool],
    pub turn_allow: Option<&'a HashSet<String>>,
    /// 本回合权限能力快照；`None` = 调用方尚未接线（等价于不额外限制）。
    pub permission: Option<&'a dyn crate::cm_agent::permission::PolicyEngine>,
}

/// 串行 / 并行路径共用的策略 early-deny（不含 TTL / run_command 预检）。
pub fn tool_policy_early_deny_message(p: &ToolPolicyEarlyDenyParams<'_>) -> Option<String> {
    // 权限等级先行：它比「角色白名单」更硬（L0/L1 下连工具定义都没下发），
    // 先拒能让模型收到「等级不允许」而不是「角色白名单不允许」这一误导性归因。
    if let Some(caps) = p.permission
        && let Some(axis) = crate::cm_agent::permission::PolicyEngine::denied_axis(caps, p.name)
    {
        // 归因与放行判定同源（端口的 `denied_axis`）：外部工具轴 → 命令类硬拒 → 可见性。
        // 调用点不再自己按 `command_scope` / 外部工具轴重算一遍，避免两处顺序漂移。
        crate::cm_agent::permission_audit::record_denied(caps, axis, Some(p.name), None);
        return Some(crate::cm_agent::permission::PolicyEngine::deny_message(
            caps, p.name,
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_config::load_config;
    use crate::cm_types::{FunctionCall, ToolCall};

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

    /// 一个**非** `PermissionCaps` 的端口实现：固定全拒。
    ///
    /// 它存在的唯一目的是证明端口的**可替换性**——等价性测试只覆盖「`PermissionCaps`
    /// 这个实现 ≡ 自由函数」，证明不了「执行面真的只认 trait、宿主能换实现」。
    struct StubDenyAll;

    impl crate::cm_agent::permission::PolicyEngine for StubDenyAll {
        fn level(&self) -> crate::cm_types::PermissionLevel {
            crate::cm_types::PermissionLevel::Read
        }

        fn approval(&self) -> crate::cm_types::ApprovalPolicy {
            crate::cm_types::ApprovalPolicy::Deny
        }

        fn path_scope(&self) -> crate::cm_types::PathScope {
            crate::cm_types::PathScope::SessionRoot
        }

        fn outbound_denied(&self) -> bool {
            true
        }

        fn outbound_allows_any(&self) -> bool {
            false
        }

        fn outbound_prefixes(&self) -> &[String] {
            &[]
        }

        fn command_allowlist(&self) -> Option<std::sync::Arc<[String]>> {
            None
        }

        fn denied_axis(
            &self,
            _name: &str,
        ) -> Option<crate::cm_agent::permission_audit::PermissionAxis> {
            Some(crate::cm_agent::permission_audit::PermissionAxis::Tool)
        }

        fn deny_message(&self, name: &str) -> String {
            format!("stub-denied:{name}")
        }
    }

    #[test]
    fn early_deny_turn_allow_blocks_tool() {
        let cfg = test_cfg();
        let mut allow = HashSet::new();
        allow.insert("read_file".to_string());
        let msg = tool_policy_early_deny_message(&ToolPolicyEarlyDenyParams {
            cfg: &cfg,
            name: "run_command",
            step_executor_constraint: None,
            tools_defs: &[],
            turn_allow: Some(&allow),
            permission: None,
        });
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("run_command"));
    }

    #[test]
    fn early_deny_follows_injected_policy_engine_not_caps() {
        let cfg = test_cfg();
        let msg = tool_policy_early_deny_message(&ToolPolicyEarlyDenyParams {
            cfg: &cfg,
            name: "read_file",
            step_executor_constraint: None,
            tools_defs: &[],
            turn_allow: None,
            permission: Some(&StubDenyAll),
        });
        // 拒因文案来自端口实现本身（`deny_message`），而不是 `permission_denied_message(caps)`：
        // 这条断言就是「执行面只认 trait」的证据。
        assert_eq!(msg.as_deref(), Some("stub-denied:read_file"));
    }
}
