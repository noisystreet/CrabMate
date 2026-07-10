//! `PlanStepExecutorKind`（分阶段 `executor_kind`）与 **DAG 节点 `node_tool_role`** 共用的工具允许表。

use std::collections::HashSet;
use std::sync::OnceLock;

use crabmate_config::AgentConfig;
use crabmate_tools::registry_policy::is_readonly_tool;
use crabmate_tools::tool_naming::is_mcp_proxy_tool;
use crabmate_types::Tool;

use crate::plan_artifact::PlanStepExecutorKind;

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
    cfg.tool_registry_policy
        .tool_registry_sub_agent_patch_write_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
}

fn test_runner_allowed_name(cfg: &AgentConfig, name: &str) -> bool {
    if default_test_runner_tool_names().contains(name) {
        return true;
    }
    cfg.tool_registry_policy
        .tool_registry_sub_agent_test_runner_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
}

/// 分阶段 `review_readonly` 步末空执行检测：须出现阅读/探查类只读工具。
pub fn tool_name_implies_readonly_probe(name: &str) -> bool {
    if is_mcp_proxy_tool(name) {
        return false;
    }
    matches!(
        name,
        "read_file"
            | "read_dir"
            | "list_dir"
            | "grep"
            | "search_in_files"
            | "file_exists"
            | "stat"
            | "git_diff"
            | "git_show"
            | "git_log"
            | "git_status"
            | "git_blame"
            | "find_references"
            | "call_graph_sketch"
            | "archive_list"
            | "http_fetch"
            | "diagnostic_summary"
            | "lizard_complexity"
            | "shellcheck_check"
            | "cppcheck_analyze"
            | "semgrep_scan"
    )
}

/// 分阶段 `patch_write` 步末空执行检测：须出现补丁/写文件类工具。
pub fn tool_name_implies_patch_write_progress(name: &str) -> bool {
    default_patch_write_tool_names().contains(name)
}

/// 该 `executor_kind` / 节点角色下是否允许调用该工具。
pub fn tool_allowed_for_step_executor_kind(
    cfg: &AgentConfig,
    name: &str,
    kind: PlanStepExecutorKind,
) -> bool {
    match kind {
        PlanStepExecutorKind::ReviewReadonly => {
            if cfg
                .tool_registry_policy
                .tool_registry_sub_agent_review_readonly_deny_tools
                .as_ref()
                .is_some_and(|s| s.contains(name))
            {
                return false;
            }
            is_readonly_tool(cfg, name) && !is_mcp_proxy_tool(name)
        }
        PlanStepExecutorKind::PatchWrite => {
            if is_mcp_proxy_tool(name) {
                return false;
            }
            is_readonly_tool(cfg, name) || patch_write_allowed_name(cfg, name)
        }
        PlanStepExecutorKind::TestRunner => {
            if is_mcp_proxy_tool(name) {
                return false;
            }
            is_readonly_tool(cfg, name) || test_runner_allowed_name(cfg, name)
        }
    }
}

/// 供模型 `tools` 列表：仅保留该步角色允许的工具定义。
pub fn filter_tool_defs_for_executor_kind(
    all: &[Tool],
    cfg: &AgentConfig,
    kind: PlanStepExecutorKind,
) -> Vec<Tool> {
    all.iter()
        .filter(|t| tool_allowed_for_step_executor_kind(cfg, t.function.name.as_str(), kind))
        .cloned()
        .collect()
}

pub fn executor_kind_user_label(kind: PlanStepExecutorKind) -> &'static str {
    match kind {
        PlanStepExecutorKind::ReviewReadonly => "review_readonly（只读审阅）",
        PlanStepExecutorKind::PatchWrite => "patch_write（只读 + 受限补丁写）",
        PlanStepExecutorKind::TestRunner => {
            "test_runner（只读 + 测试/构建工具 + run_command 白名单）"
        }
    }
}

/// 越权拒绝时写入 `role: tool` 的正文（含角色说明与允许工具名摘要）。
pub fn executor_kind_tool_denied_body(
    cfg: &AgentConfig,
    tools_defs: &[Tool],
    name: &str,
    kind: PlanStepExecutorKind,
) -> String {
    let label = executor_kind_user_label(kind);
    let hint = allowed_tools_hint_csv(cfg, kind, tools_defs);
    format!("工具「{name}」不在本步子代理角色 {label} 的允许范围内。{hint}")
}

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

    use crabmate_config::load_config;
    use crabmate_types::FunctionDef;

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
        let mut cfg = load_config(None).expect("embed default");
        let mut writes = cfg
            .tool_registry_policy
            .tool_registry_write_effect_tools
            .as_ref()
            .map(|a| a.as_ref().clone())
            .unwrap_or_default();
        writes.insert("my_patch".to_string());
        cfg.tool_registry_policy.tool_registry_write_effect_tools = Some(Arc::new(writes));
        cfg.tool_registry_policy
            .tool_registry_sub_agent_patch_write_extra_tools =
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
        let mut cfg = load_config(None).expect("embed default");
        cfg.tool_registry_policy
            .tool_registry_sub_agent_review_readonly_deny_tools =
            Some(Arc::new(HashSet::from(["read_file".to_string()])));
        assert!(!tool_allowed_for_step_executor_kind(
            &cfg,
            "read_file",
            PlanStepExecutorKind::ReviewReadonly
        ));
    }

    #[test]
    fn test_runner_allows_run_command_under_allowlist_semantics() {
        let cfg = load_config(None).expect("embed default");
        assert!(tool_allowed_for_step_executor_kind(
            &cfg,
            "run_command",
            PlanStepExecutorKind::TestRunner
        ));
    }

    #[test]
    fn denied_body_lists_allowed_names() {
        let cfg = load_config(None).expect("embed default");
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
