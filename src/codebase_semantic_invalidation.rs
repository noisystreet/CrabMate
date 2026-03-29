//! 工作区写操作后使代码语义索引失效（删除 SQLite 中对应块或整表），与 `read_file` 缓存清空语义对齐。

use std::path::Path;

use rusqlite::params;

use crate::codebase_semantic_index::{index_path_for_workspace, open_codebase_semantic_db};
use crate::tool_result::parse_legacy_output;
use crate::tools::canonical_workspace_root;

const CHUNKS_TABLE: &str = "crabmate_codebase_chunks";

#[derive(Debug, Clone)]
pub(crate) enum CodebaseSemanticInvalidation {
    /// 删除当前 `workspace_root` 键下全部块。
    FullWorkspace,
    /// 删除这些相对路径（POSIX）下的所有块。
    RelPaths(Vec<String>),
}

/// 根据工具名与参数推断应失效的范围；**只读工具**返回 `None`。
pub(crate) fn invalidation_for_tool_call(
    name: &str,
    args_json: &str,
) -> Option<CodebaseSemanticInvalidation> {
    if crate::tool_registry::is_readonly_tool(name) {
        return None;
    }

    // 子进程 / 工作流 / 网络写：无法可靠解析受影响路径 → 整表失效。
    if matches!(
        name,
        "run_command"
            | "run_executable"
            | "playbook_run_commands"
            | "workflow_execute"
            | "http_request"
            | "cargo_fix"
            | "cargo_clean"
            | "python_install_editable"
            | "npm_install"
            | "go_mod_tidy"
    ) || name.starts_with("git_")
    {
        return Some(CodebaseSemanticInvalidation::FullWorkspace);
    }

    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;

    let mut paths: Vec<String> = Vec::new();
    let mut push = |s: Option<&str>| {
        if let Some(t) = s.map(str::trim).filter(|x| !x.is_empty()) {
            paths.push(t.replace('\\', "/"));
        }
    };

    match name {
        "create_file" | "modify_file" | "delete_file" | "delete_dir" | "append_file"
        | "create_dir" | "search_replace" | "chmod_file" | "format_file" | "format_check_file"
        | "extract_in_file" | "read_binary_meta" | "hash_file" => {
            push(v.get("path").and_then(|p| p.as_str()));
        }
        "copy_file" | "move_file" => {
            push(v.get("from").and_then(|p| p.as_str()));
            push(v.get("to").and_then(|p| p.as_str()));
        }
        "apply_patch" => {
            if let Some(patch) = v.get("patch").and_then(|p| p.as_str()) {
                paths.extend(patch_paths_from_unified_diff(patch));
            }
            if paths.is_empty() {
                return Some(CodebaseSemanticInvalidation::FullWorkspace);
            }
        }
        "structured_patch" | "markdown_check_links" | "typos_check" | "codespell_check" => {
            push(v.get("path").and_then(|p| p.as_str()));
            if name == "markdown_check_links"
                && let Some(roots) = v.get("roots").and_then(|r| r.as_array())
            {
                for x in roots {
                    push(x.as_str());
                }
            }
            if matches!(name, "typos_check" | "codespell_check")
                && let Some(ps) = v.get("paths").and_then(|p| p.as_array())
            {
                for x in ps {
                    push(x.as_str());
                }
            }
        }
        "ast_grep_rewrite" => {
            if let Some(ps) = v.get("paths").and_then(|p| p.as_array()) {
                for x in ps {
                    push(x.as_str());
                }
            } else {
                return Some(CodebaseSemanticInvalidation::FullWorkspace);
            }
        }
        _ => {
            return Some(CodebaseSemanticInvalidation::FullWorkspace);
        }
    }

    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        Some(CodebaseSemanticInvalidation::FullWorkspace)
    } else {
        Some(CodebaseSemanticInvalidation::RelPaths(paths))
    }
}

fn patch_paths_from_unified_diff(patch: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("--- ") {
            let path_part = rest.split_whitespace().next().unwrap_or(rest);
            let path_part = path_part.strip_prefix("a/").unwrap_or(path_part);
            if path_part == "/dev/null" || path_part.is_empty() {
                continue;
            }
            if path_part.starts_with("b/") {
                continue;
            }
            out.push(path_part.replace('\\', "/"));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 工具返回串视为成功时使索引失效（与 `read_file` 缓存清空条件一致：`!is_readonly || workspace_changed` 之后调用本函数时应对**当前**写工具传入 `ok`）。
pub(crate) fn apply_after_successful_tool(
    workspace_root: &Path,
    index_sqlite_path_cfg: &str,
    inv: CodebaseSemanticInvalidation,
) {
    let ws_key = match canonical_workspace_root(workspace_root) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => return,
    };
    let index_path = match index_path_for_workspace(workspace_root, index_sqlite_path_cfg) {
        Ok(p) => p,
        Err(_) => return,
    };
    let mut conn = match open_codebase_semantic_db(&index_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    match inv {
        CodebaseSemanticInvalidation::FullWorkspace => {
            let _ = conn.execute(
                &format!("DELETE FROM {CHUNKS_TABLE} WHERE workspace_root = ?1"),
                params![ws_key],
            );
        }
        CodebaseSemanticInvalidation::RelPaths(paths) => {
            if paths.is_empty() {
                return;
            }
            let tx = match conn.transaction() {
                Ok(t) => t,
                Err(_) => return,
            };
            let mut ok = true;
            for rel in &paths {
                if tx
                    .execute(
                        &format!(
                            "DELETE FROM {CHUNKS_TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"
                        ),
                        params![ws_key, rel],
                    )
                    .is_err()
                {
                    ok = false;
                    break;
                }
            }
            if ok {
                let _ = tx.commit();
            }
        }
    }
}

/// `run_tool` 成功语义：`crabmate_tool` 信封的 `ok`，或旧式解析的 `ok`。
pub(crate) fn tool_output_semantic_success(tool_name: &str, output: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(output)
        && let Some(ct) = v.get("crabmate_tool").and_then(|x| x.as_object())
    {
        return ct.get("ok").and_then(|x| x.as_bool()) != Some(false);
    }
    parse_legacy_output(tool_name, output).ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_paths_parses_git_style() {
        let p = r#"--- a/src/foo.rs
+++ b/src/foo.rs
@@ -1 +1 @@
"#;
        let v = patch_paths_from_unified_diff(p);
        assert!(v.iter().any(|s| s.ends_with("src/foo.rs")));
    }

    #[test]
    fn run_command_invalidates_full() {
        let inv = invalidation_for_tool_call("run_command", r#"{"command":"touch","args":["x"]}"#);
        assert!(matches!(
            inv,
            Some(CodebaseSemanticInvalidation::FullWorkspace)
        ));
    }

    #[test]
    fn read_file_no_invalidation() {
        assert!(invalidation_for_tool_call("read_file", r#"{"path":"a.rs"}"#).is_none());
    }
}
