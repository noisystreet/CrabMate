//! 分阶段规划「子代理」步级工具约束：规划 JSON 可选 `executor_kind`，执行层收窄可见工具并拒绝越权调用。

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::config::AgentConfig;
use crate::tool_registry;
use crate::types::Tool;

use crate::agent::plan_artifact::PlanStepExecutorKind;

fn patch_write_tool_names() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "apply_patch",
            "search_replace",
            "structured_patch",
            "create_file",
            "modify_file",
            "append_file",
            "format_file",
            "ast_grep_rewrite",
        ]
        .into_iter()
        .collect()
    })
}

fn test_runner_tool_names() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "cargo_test",
            "cargo_nextest",
            "rust_test_one",
            "pytest_run",
            "go_test",
            "maven_test",
            "gradle_test",
            "frontend_test",
        ]
        .into_iter()
        .collect()
    })
}

/// 步级子循环是否允许调用该工具（**不**改变 `run_command` / MCP 等既有审批语义；仅做名单过滤）。
pub(crate) fn tool_allowed_for_step_executor_kind(
    cfg: &AgentConfig,
    name: &str,
    kind: PlanStepExecutorKind,
) -> bool {
    match kind {
        PlanStepExecutorKind::ReviewReadonly => {
            tool_registry::is_readonly_tool(cfg, name) && !crate::mcp::is_mcp_proxy_tool(name)
        }
        PlanStepExecutorKind::PatchWrite => {
            if crate::mcp::is_mcp_proxy_tool(name) {
                return false;
            }
            tool_registry::is_readonly_tool(cfg, name) || patch_write_tool_names().contains(name)
        }
        PlanStepExecutorKind::TestRunner => {
            if crate::mcp::is_mcp_proxy_tool(name) {
                return false;
            }
            tool_registry::is_readonly_tool(cfg, name) || test_runner_tool_names().contains(name)
        }
    }
}

/// 供模型 `tools` 列表：仅保留该步角色允许的工具定义。
pub(crate) fn filter_tool_defs_for_executor_kind(
    all: &[Tool],
    cfg: &AgentConfig,
    kind: PlanStepExecutorKind,
) -> Vec<Tool> {
    all.iter()
        .filter(|t| tool_allowed_for_step_executor_kind(cfg, t.function.name.as_str(), kind))
        .cloned()
        .collect()
}
