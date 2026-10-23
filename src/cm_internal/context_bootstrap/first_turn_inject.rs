//! Web / CLI 首轮注入合并（依赖 `memory` / `living_docs` / 依赖摘要，保留在 internal）。
//! 项目画像（`project_profile`）**不在首轮注入**：仅由 `GET /workspace/profile` 与
//! `repo_overview_sweep` 按需生成，避免每次新会话都跑全仓扫描 + `cargo metadata`。

use std::path::Path;

use crate::cm_config::AgentConfig;

use super::project_dependency_brief;
use crate::cm_internal::memory::agent_memory;

fn inject_flag_with_budget(enabled: bool, max_chars: usize) -> bool {
    enabled && max_chars > 0
}

fn first_turn_memory_snippet(
    workspace_root: &Path,
    cfg: &AgentConfig,
    memory_preloaded: Option<String>,
) -> Option<String> {
    memory_preloaded.or_else(|| {
        if cfg.context_bootstrap_inject.agent_memory_file_enabled {
            agent_memory::load_memory_snippet(
                workspace_root,
                cfg.context_bootstrap_inject.agent_memory_file.as_str(),
                cfg.context_bootstrap_inject.agent_memory_file_max_chars,
            )
        } else {
            None
        }
    })
}

fn first_turn_living_snippet(workspace_root: &Path, cfg: &AgentConfig) -> Option<String> {
    if !inject_flag_with_budget(
        cfg.context_bootstrap_inject.living_docs_inject_enabled,
        cfg.context_bootstrap_inject.living_docs_inject_max_chars,
    ) {
        return None;
    }
    super::living_docs::load_living_docs_snippet(
        workspace_root,
        cfg.context_bootstrap_inject
            .living_docs_relative_dir
            .as_str(),
        cfg.context_bootstrap_inject.living_docs_inject_max_chars,
        cfg.context_bootstrap_inject.living_docs_file_max_each_chars,
    )
}

/// Web / CLI 首轮：合并备忘、活文档摘要、依赖摘要；全无则 `None`。
pub fn build_first_turn_user_context_markdown(
    workspace_root: &Path,
    cfg: &AgentConfig,
    memory_preloaded: Option<String>,
) -> Option<String> {
    if workspace_root.as_os_str().is_empty() {
        return None;
    }
    let memory_snippet = first_turn_memory_snippet(workspace_root, cfg, memory_preloaded);
    let living_snippet = first_turn_living_snippet(workspace_root, cfg);
    let want_dep = inject_flag_with_budget(
        cfg.context_bootstrap_inject
            .project_dependency_brief_inject_enabled,
        cfg.context_bootstrap_inject
            .project_dependency_brief_inject_max_chars,
    );
    if !want_dep && memory_snippet.is_none() && living_snippet.is_none() {
        return None;
    }
    let dep_md = if want_dep {
        project_dependency_brief::build_project_dependency_brief_markdown(
            workspace_root,
            cfg.context_bootstrap_inject
                .project_dependency_brief_inject_max_chars,
        )
    } else {
        String::new()
    };
    merge_first_turn_injections(
        living_snippet.as_deref(),
        memory_snippet.as_deref(),
        dep_md.as_str(),
    )
}

/// 合并活文档摘要、备忘、依赖结构摘要为一条首轮 `user` 正文。
pub fn merge_first_turn_injections(
    living_docs_snippet: Option<&str>,
    memory_snippet: Option<&str>,
    dependency_brief_markdown: &str,
) -> Option<String> {
    let living = living_docs_snippet.map(str::trim).filter(|s| !s.is_empty());
    let mem = memory_snippet.map(str::trim).filter(|s| !s.is_empty());
    let dep = dependency_brief_markdown.trim();
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = living {
        parts.push(m.to_string());
    }
    if let Some(m) = mem {
        parts.push(m.to_string());
    }
    if !dep.is_empty() {
        parts.push(format!(
            "[项目依赖与结构摘要（cargo metadata + package.json，仅只读）]\n{dep}"
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_first_turn_two_parts() {
        let got = merge_first_turn_injections(None, Some("memo line"), "## Dep\nx").expect("some");
        assert!(got.contains("memo line"));
        assert!(got.contains("依赖与结构摘要"));
        // 回归守卫：项目画像不再进入首轮注入。
        assert!(!got.contains("项目画像"));
    }
}
