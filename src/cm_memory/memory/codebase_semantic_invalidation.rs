//! 工作区写操作后使代码语义索引失效（删除 SQLite 中对应块或整表），与 `read_file` 缓存清空语义对齐。

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use rusqlite::params;

use crate::cm_memory::memory::codebase_semantic_index::{
    CODEBASE_SEMANTIC_FILES_TABLE, index_path_for_workspace, open_codebase_semantic_db,
};
use crate::cm_config::AgentConfig;
use crate::cm_types::path_utils::canonical_workspace_root;

use crate::cm_memory::tool_check;

const CHUNKS_TABLE: &str = "crabmate_codebase_chunks";

/// 内建写副作用工具名表，用于判断工具是否只读。
fn builtin_write_effect_tools() -> HashSet<&'static str> {
    HashSet::from([
        "apply_diff",
        "create_file",
        "write_file",
        "edit_file",
        "edit_and_apply",
        "delete_dir",
        "delete_files",
        "move_file",
        "copy_file",
        "create_symlink",
        "format_file",
        "format_check_file",
        "set_env_var",
        "set_dot_env_var",
        "upsert_secret_file",
        "append_lines",
        "create_symbolic_link",
        "write",
        "mkdir",
        "git_add",
        "git_commit",
        "git_push",
        "git_revert",
        "git_stash",
        "git_reset",
        "git_clone",
        "git_fetch",
        "cargo_fix",
        "cargo_clean",
        "python_install_editable",
        "npm_install",
        "go_mod_tidy",
        "docker_build",
        "long_term_remember",
        "long_term_forget",
        "run_command",
        "terminal_session",
        "playbook_run_commands",
        "python_snippet_run",
        "run_executable",
        "workflow_execute",
        "http_request",
        "gh_api",
        "gh_pr_create",
        "gh_pr_merge",
        "gh_pr_review",
        "gh_pr_comment",
        "gh_issue_create",
        "gh_run_rerun",
        "gh_release_create",
    ])
}

/// 判断工具是否为只读（不修改工作区文件系统），供失效决策使用。
/// 写操作工具及未知语义工具（MCP 代理、动态工具）返回 false。
fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    static BUILTIN: LazyLock<HashSet<String>> = LazyLock::new(|| {
        builtin_write_effect_tools()
            .into_iter()
            .map(String::from)
            .collect()
    });
    let writes = match &cfg.tool_registry_policy.tool_registry_write_effect_tools {
        None => &BUILTIN,
        Some(arc) => arc.as_ref(),
    };
    // MCP 代理工具与动态工具语义不可静态证明，默认按写副作用处理。
    if name.starts_with("mcp__") || name.starts_with("tool_") {
        return false;
    }
    !writes.contains(name)
}

/// 相对工作区路径（POSIX，`/`）；`is_dir` 为 true 时删除该路径及其子路径下所有块。
#[derive(Debug, Clone)]
pub struct RelScope {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub enum CodebaseSemanticInvalidation {
    /// 删除当前 `workspace_root` 键下全部块。
    FullWorkspace,
    /// 按路径或目录前缀删除块。
    RelScopes(Vec<RelScope>),
}

fn push_invalidation_scope(scopes: &mut Vec<RelScope>, s: Option<&str>, is_dir: bool) {
    if let Some(t) = s.map(str::trim).filter(|x| !x.is_empty()) {
        scopes.push(RelScope {
            path: t.replace('\\', "/"),
            is_dir,
        });
    }
}

enum InvalScopesFill {
    Ok,
    FullWorkspace,
}

fn fill_dir_path_tools(name: &str, v: &serde_json::Value, scopes: &mut Vec<RelScope>) -> Option<InvalScopesFill> {
    if !matches!(name, "delete_dir" | "create_dir") {
        return None;
    }
    push_invalidation_scope(scopes, v.get("path").and_then(|p| p.as_str()), true);
    Some(InvalScopesFill::Ok)
}

fn is_single_file_path_tool(name: &str) -> bool {
    matches!(
        name,
        "create_file"
            | "modify_file"
            | "delete_files"
            | "append_file"
            | "search_replace"
            | "chmod_file"
            | "format_file"
            | "format_check_file"
            | "extract_in_file"
            | "read_binary_meta"
            | "hash_file"
    )
}

fn fill_single_file_path_tools(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> Option<InvalScopesFill> {
    if !is_single_file_path_tool(name) {
        return None;
    }
    push_invalidation_scope(scopes, v.get("path").and_then(|p| p.as_str()), false);
    Some(InvalScopesFill::Ok)
}

fn fill_copy_move_tools(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> Option<InvalScopesFill> {
    if !matches!(name, "copy_file" | "move_file") {
        return None;
    }
    push_invalidation_scope(scopes, v.get("from").and_then(|p| p.as_str()), false);
    push_invalidation_scope(scopes, v.get("to").and_then(|p| p.as_str()), false);
    Some(InvalScopesFill::Ok)
}

fn fill_apply_patch_tool(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> Option<InvalScopesFill> {
    if name != "apply_patch" {
        return None;
    }
    if let Some(patch) = v.get("patch").and_then(|p| p.as_str()) {
        for rel in patch_paths_from_unified_diff(patch) {
            push_invalidation_scope(scopes, Some(rel.as_str()), false);
        }
    }
    if scopes.is_empty() {
        Some(InvalScopesFill::FullWorkspace)
    } else {
        Some(InvalScopesFill::Ok)
    }
}

fn fill_structured_spell_tools(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> Option<InvalScopesFill> {
    if !matches!(
        name,
        "structured_patch" | "markdown_check_links" | "typos_check" | "codespell_check"
    ) {
        return None;
    }
    push_invalidation_scope(scopes, v.get("path").and_then(|p| p.as_str()), false);
    if name == "markdown_check_links"
        && let Some(roots) = v.get("roots").and_then(|r| r.as_array())
    {
        for x in roots {
            push_invalidation_scope(scopes, x.as_str(), false);
        }
    }
    if matches!(name, "typos_check" | "codespell_check")
        && let Some(ps) = v.get("paths").and_then(|p| p.as_array())
    {
        for x in ps {
            push_invalidation_scope(scopes, x.as_str(), false);
        }
    }
    Some(InvalScopesFill::Ok)
}

fn fill_ast_grep_tool(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> Option<InvalScopesFill> {
    if name != "ast_grep_rewrite" {
        return None;
    }
    if let Some(ps) = v.get("paths").and_then(|p| p.as_array()) {
        for x in ps {
            push_invalidation_scope(scopes, x.as_str(), false);
        }
        Some(InvalScopesFill::Ok)
    } else {
        Some(InvalScopesFill::FullWorkspace)
    }
}

fn fill_invalidation_scopes_for_tool(
    name: &str,
    v: &serde_json::Value,
    scopes: &mut Vec<RelScope>,
) -> InvalScopesFill {
    if let Some(r) = fill_dir_path_tools(name, v, scopes) {
        return r;
    }
    if let Some(r) = fill_single_file_path_tools(name, v, scopes) {
        return r;
    }
    if let Some(r) = fill_copy_move_tools(name, v, scopes) {
        return r;
    }
    if let Some(r) = fill_apply_patch_tool(name, v, scopes) {
        return r;
    }
    if let Some(r) = fill_structured_spell_tools(name, v, scopes) {
        return r;
    }
    if let Some(r) = fill_ast_grep_tool(name, v, scopes) {
        return r;
    }
    InvalScopesFill::FullWorkspace
}

/// 根据工具名与参数推断应失效的范围；**只读工具**返回 `None`。
pub fn invalidation_for_tool_call(
    cfg: &AgentConfig,
    name: &str,
    args_json: &str,
) -> Option<CodebaseSemanticInvalidation> {
    if is_readonly_tool(cfg, name) {
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

    let mut scopes: Vec<RelScope> = Vec::new();

    match fill_invalidation_scopes_for_tool(name, &v, &mut scopes) {
        InvalScopesFill::Ok => {}
        InvalScopesFill::FullWorkspace => return Some(CodebaseSemanticInvalidation::FullWorkspace),
    }

    scopes.sort_by(|a, b| a.path.cmp(&b.path));
    scopes.dedup_by(|a, b| {
        if a.path != b.path {
            return false;
        }
        a.is_dir |= b.is_dir;
        true
    });
    if scopes.is_empty() {
        Some(CodebaseSemanticInvalidation::FullWorkspace)
    } else {
        Some(CodebaseSemanticInvalidation::RelScopes(scopes))
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
pub fn apply_after_successful_tool(
    workspace_root: &Path,
    index_sqlite_path_cfg: &str,
    inv: CodebaseSemanticInvalidation,
) {
    let Some((ws_key, mut conn)) = open_invalidation_connection(workspace_root, index_sqlite_path_cfg)
    else {
        return;
    };
    match inv {
        CodebaseSemanticInvalidation::FullWorkspace => {
            delete_workspace_index_rows(&conn, &ws_key);
        }
        CodebaseSemanticInvalidation::RelScopes(scopes) => {
            apply_rel_scope_invalidation(&mut conn, &ws_key, &scopes);
        }
    }
}

fn open_invalidation_connection(
    workspace_root: &Path,
    index_sqlite_path_cfg: &str,
) -> Option<(String, rusqlite::Connection)> {
    let ws_key = canonical_workspace_root(workspace_root)
        .ok()?
        .to_string_lossy()
        .to_string();
    let index_path = index_path_for_workspace(workspace_root, index_sqlite_path_cfg).ok()?;
    let conn = open_codebase_semantic_db(&index_path).ok()?;
    Some((ws_key, conn))
}

fn delete_workspace_index_rows(conn: &rusqlite::Connection, ws_key: &str) {
    let _ = conn.execute(
        &format!("DELETE FROM {CHUNKS_TABLE} WHERE workspace_root = ?1"),
        params![ws_key],
    );
    let _ = conn.execute(
        &format!("DELETE FROM {CODEBASE_SEMANTIC_FILES_TABLE} WHERE workspace_root = ?1"),
        params![ws_key],
    );
}

fn delete_one_rel_scope(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    sc: &RelScope,
) -> rusqlite::Result<()> {
    if sc.is_dir {
        let like_pat = sqlite_like_escape(&format!("{}/%", sc.path.trim_end_matches('/')));
        let path = sc.path.trim_end_matches('/');
        tx.execute(
            &format!(
                "DELETE FROM {CHUNKS_TABLE} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
            ),
            params![ws_key, path, like_pat],
        )?;
        tx.execute(
            &format!(
                "DELETE FROM {CODEBASE_SEMANTIC_FILES_TABLE} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
            ),
            params![ws_key, path, like_pat],
        )?;
        return Ok(());
    }
    tx.execute(
        &format!("DELETE FROM {CHUNKS_TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
        params![ws_key, sc.path.as_str()],
    )?;
    tx.execute(
        &format!(
            "DELETE FROM {CODEBASE_SEMANTIC_FILES_TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"
        ),
        params![ws_key, sc.path.as_str()],
    )?;
    Ok(())
}

fn apply_rel_scope_invalidation(
    conn: &mut rusqlite::Connection,
    ws_key: &str,
    scopes: &[RelScope],
) {
    if scopes.is_empty() {
        return;
    }
    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(_) => return,
    };
    let mut ok = true;
    for sc in scopes {
        if delete_one_rel_scope(&tx, ws_key, sc).is_err() {
            ok = false;
            break;
        }
    }
    if ok {
        let _ = tx.commit();
    }
}

fn sqlite_like_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(ch);
    }
    o
}

/// `run_tool` 成功语义：`crabmate_tool` 信封的 `ok`，或旧式解析的 `ok`。
pub fn tool_output_semantic_success(_tool_name: &str, output: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(output)
        && let Some(ct) = v.get("crabmate_tool").and_then(|x| x.as_object())
    {
        return ct.get("ok").and_then(|x| x.as_bool()) != Some(false);
    }
    tool_check::tool_output_is_ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> AgentConfig {
        crate::cm_config::load_config(None).expect("embed default")
    }

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
        let cfg = default_cfg();
        let inv =
            invalidation_for_tool_call(&cfg, "run_command", r#"{"command":"touch","args":["x"]}"#);
        assert!(matches!(
            inv,
            Some(CodebaseSemanticInvalidation::FullWorkspace)
        ));
    }

    #[test]
    fn read_file_no_invalidation() {
        let cfg = default_cfg();
        assert!(invalidation_for_tool_call(&cfg, "read_file", r#"{"path":"a.rs"}"#).is_none());
    }

    #[test]
    fn delete_dir_uses_prefix_scope() {
        let cfg = default_cfg();
        let inv = invalidation_for_tool_call(&cfg, "delete_dir", r#"{"path":"src/lib"}"#);
        let Some(CodebaseSemanticInvalidation::RelScopes(sc)) = inv else {
            panic!("expected RelScopes");
        };
        assert_eq!(sc.len(), 1);
        assert!(sc[0].is_dir);
        assert_eq!(sc[0].path, "src/lib");
    }

    #[test]
    fn create_file_is_file_scope() {
        let cfg = default_cfg();
        let inv = invalidation_for_tool_call(&cfg, "create_file", r#"{"path":"a/b.rs"}"#);
        let Some(CodebaseSemanticInvalidation::RelScopes(sc)) = inv else {
            panic!("expected RelScopes");
        };
        assert_eq!(sc.len(), 1);
        assert!(!sc[0].is_dir);
        assert_eq!(sc[0].path, "a/b.rs");
    }
}
