//! 分阶段规划「子代理」步级工具约束：规划 JSON 可选 `executor_kind`，执行层收窄可见工具并拒绝越权调用。
//!
//! `patch_write` 的补丁类工具集与 **`[tool_registry] write_effect_tools`** 对齐思路：只读性由 `is_readonly_tool` 判定；补丁名来自内建默认并可由 **`sub_agent_patch_write_extra_tools`** 扩充（默认配置已包含 **`run_command`**，便于在补丁步骤中使用 `git` 等版本控制命令）。`test_runner` 有内建测试工具集、默认包含 **`run_command`**（具体命令仍仅能为配置白名单所允许），并可由 **`sub_agent_test_runner_extra_tools`** 扩充。

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::config::AgentConfig;
use crate::tool_registry;
use crate::types::Tool;

use crate::agent::plan_artifact::PlanStepExecutorKind;

fn default_patch_write_tool_names() -> &'static HashSet<&'static str> {
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

fn default_test_runner_tool_names() -> &'static HashSet<&'static str> {
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
            // 编译 / 检查等仍走白名单，与全局 `run_command` 一致
            "run_command",
        ]
        .into_iter()
        .collect()
    })
}

fn patch_write_allowed_name(cfg: &AgentConfig, name: &str) -> bool {
    if default_patch_write_tool_names().contains(name) {
        return true;
    }
    cfg.tool_registry_sub_agent_patch_write_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
}

fn test_runner_allowed_name(cfg: &AgentConfig, name: &str) -> bool {
    if default_test_runner_tool_names().contains(name) {
        return true;
    }
    cfg.tool_registry_sub_agent_test_runner_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
}

/// 步级子循环是否允许调用该工具（**不**改变 `run_command` / MCP 等既有审批语义；仅做名单过滤）。
pub(crate) fn tool_allowed_for_step_executor_kind(
    cfg: &AgentConfig,
    name: &str,
    kind: PlanStepExecutorKind,
) -> bool {
    match kind {
        PlanStepExecutorKind::ReviewReadonly => {
            if cfg
                .tool_registry_sub_agent_review_readonly_deny_tools
                .as_ref()
                .is_some_and(|s| s.contains(name))
            {
                return false;
            }
            tool_registry::is_readonly_tool(cfg, name) && !crate::mcp::is_mcp_proxy_tool(name)
        }
        PlanStepExecutorKind::PatchWrite => {
            if crate::mcp::is_mcp_proxy_tool(name) {
                return false;
            }
            tool_registry::is_readonly_tool(cfg, name) || patch_write_allowed_name(cfg, name)
        }
        PlanStepExecutorKind::TestRunner => {
            if crate::mcp::is_mcp_proxy_tool(name) {
                return false;
            }
            tool_registry::is_readonly_tool(cfg, name) || test_runner_allowed_name(cfg, name)
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

pub(crate) fn executor_kind_user_label(kind: PlanStepExecutorKind) -> &'static str {
    match kind {
        PlanStepExecutorKind::ReviewReadonly => "review_readonly（只读审阅）",
        PlanStepExecutorKind::PatchWrite => "patch_write（只读 + 受限补丁写）",
        PlanStepExecutorKind::TestRunner => {
            "test_runner（只读 + 测试/构建工具 + run_command 白名单）"
        }
    }
}

/// 越权拒绝时写入 `role: tool` 的正文（含角色说明与允许工具名摘要）。
pub(crate) fn executor_kind_tool_denied_body(
    cfg: &AgentConfig,
    tools_defs: &[Tool],
    name: &str,
    kind: PlanStepExecutorKind,
) -> String {
    let label = executor_kind_user_label(kind);
    let hint = allowed_tools_hint_csv(cfg, kind, tools_defs);
    format!("工具「{name}」不在本步子代理角色 {label} 的允许范围内。{hint}")
}

/// 本步在当前会话 `tools_defs` 下允许的工具名 CSV（截断长度）。
fn allowed_tools_hint_csv(
    cfg: &AgentConfig,
    kind: PlanStepExecutorKind,
    tools_defs: &[Tool],
) -> String {
    const MAX_CHARS: usize = 900;
    let mut names: Vec<String> = tools_defs
        .iter()
        .map(|t| t.function.name.clone())
        .filter(|n| tool_allowed_for_step_executor_kind(cfg, n.as_str(), kind))
        .collect();
    names.sort();
    if names.is_empty() {
        return "当前会话工具列表中无符合该角色的工具；请让规划器省略 executor_kind 或调整步骤。"
            .to_string();
    }
    let mut csv = names.join(", ");
    if csv.chars().count() > MAX_CHARS {
        let mut out = String::new();
        for (i, n) in names.iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            if out.chars().count() + sep.chars().count() + n.chars().count() > MAX_CHARS {
                out.push('…');
                break;
            }
            out.push_str(sep);
            out.push_str(n);
        }
        csv = out;
    }
    format!("本步允许的工具名包括：{csv}。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    use crate::types::{FunctionDef, Tool};

    fn tool_named(name: &str) -> Tool {
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description: String::new(),
                parameters: serde_json::json!({}),
            },
        }
    }

    #[test]
    fn patch_write_extra_tools_from_config() {
        let mut cfg = crate::config::load_config(None).expect("embed default");
        // 将 `my_patch` 标为写副作用，使其在 review_readonly 下被拒；patch_write 仍可通过 extra 名单放行。
        let mut writes = cfg
            .tool_registry_write_effect_tools
            .as_ref()
            .map(|a| a.as_ref().clone())
            .unwrap_or_default();
        writes.insert("my_patch".to_string());
        cfg.tool_registry_write_effect_tools = Some(Arc::new(writes));
        cfg.tool_registry_sub_agent_patch_write_extra_tools =
            Some(Arc::new(HashSet::from(["my_patch".to_string()])));
        assert!(tool_allowed_for_step_executor_kind(
            &cfg,
            "my_patch",
            PlanStepExecutorKind::PatchWrite
        ));
        assert!(!tool_allowed_for_step_executor_kind(
            &cfg,
            "my_patch",
            PlanStepExecutorKind::ReviewReadonly
        ));
    }

    #[test]
    fn review_readonly_deny_tools_override() {
        let mut cfg = crate::config::load_config(None).expect("embed default");
        cfg.tool_registry_sub_agent_review_readonly_deny_tools =
            Some(Arc::new(HashSet::from(["read_file".to_string()])));
        assert!(!tool_allowed_for_step_executor_kind(
            &cfg,
            "read_file",
            PlanStepExecutorKind::ReviewReadonly
        ));
    }

    #[test]
    fn test_runner_allows_run_command_under_allowlist_semantics() {
        let cfg = crate::config::load_config(None).expect("embed default");
        assert!(tool_allowed_for_step_executor_kind(
            &cfg,
            "run_command",
            PlanStepExecutorKind::TestRunner
        ));
    }

    #[test]
    fn denied_body_lists_allowed_names() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let defs = vec![tool_named("read_file"), tool_named("create_file")];
        let body = executor_kind_tool_denied_body(
            &cfg,
            &defs,
            "create_file",
            PlanStepExecutorKind::ReviewReadonly,
        );
        assert!(body.contains("read_file"), "body={body}");
        assert!(body.contains("不在本步子代理"));
    }
}
