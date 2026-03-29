/// Dynamic summary helpers referenced by `ToolSpec::summary` `Dynamic` variants.
/// Each extracts key arguments from parsed `serde_json::Value` into a short English one-liner.
pub(super) fn summary_codebase_semantic_search(v: &serde_json::Value) -> Option<String> {
    if v.get("rebuild_index").and_then(|b| b.as_bool()) == Some(true) {
        let p = v.get("path").and_then(|x| x.as_str()).unwrap_or(".");
        return Some(format!("semantic index rebuild ({})", p));
    }
    let q = v.get("query").and_then(|x| x.as_str()).unwrap_or("");
    let t = q.trim();
    if t.is_empty() {
        Some("semantic code search".to_string())
    } else {
        let mut s = t.chars().take(48).collect::<String>();
        if t.chars().count() > 48 {
            s.push('…');
        }
        Some(format!("semantic search: {}", s))
    }
}

pub(super) fn summary_run_command(v: &serde_json::Value) -> Option<String> {
    let cmd = v.get("command")?.as_str()?.trim();
    let args = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(cmd.to_string())
    } else {
        Some(format!("{} {}", cmd, args))
    }
}

pub(super) fn summary_rust_analyzer_goto_definition(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let line = v.get("line").and_then(|x| x.as_u64());
    Some(format!(
        "rust-analyzer goto definition {}:{}",
        path,
        line.unwrap_or(0)
    ))
}

pub(super) fn summary_rust_analyzer_find_references(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let line = v.get("line").and_then(|x| x.as_u64());
    Some(format!(
        "rust-analyzer find references {}:{}",
        path,
        line.unwrap_or(0)
    ))
}

pub(super) fn summary_rust_analyzer_hover(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let line = v.get("line").and_then(|x| x.as_u64());
    Some(format!(
        "rust-analyzer hover {}:{}",
        path,
        line.unwrap_or(0)
    ))
}

pub(super) fn summary_rust_analyzer_document_symbol(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("rust-analyzer document symbols {}", path))
}

pub(super) fn summary_python_install_editable(v: &serde_json::Value) -> Option<String> {
    let b = v.get("backend").and_then(|x| x.as_str()).unwrap_or("?");
    Some(format!("editable Python install ({})", b))
}

pub(super) fn summary_uv_run(v: &serde_json::Value) -> Option<String> {
    let args = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some("uv run".to_string())
    } else {
        Some(format!("uv run {}", args))
    }
}

pub(super) fn summary_error_output_playbook(v: &serde_json::Value) -> Option<String> {
    let eco = v
        .get("ecosystem")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    Some(format!("error playbook ({})", eco))
}

pub(super) fn summary_pre_commit_run(v: &serde_json::Value) -> Option<String> {
    let hook = v.get("hook").and_then(|x| x.as_str()).unwrap_or("");
    if hook.is_empty() {
        Some("pre-commit run".to_string())
    } else {
        Some(format!("pre-commit run {}", hook))
    }
}

pub(super) fn summary_ast_grep_run(v: &serde_json::Value) -> Option<String> {
    let lang = v.get("lang").and_then(|x| x.as_str()).unwrap_or("?");
    let p = v.get("pattern").and_then(|x| x.as_str()).unwrap_or("");
    let short = if p.chars().count() > 48 {
        format!("{}…", p.chars().take(48).collect::<String>())
    } else {
        p.to_string()
    };
    Some(format!("ast-grep [{}] {}", lang, short))
}

pub(super) fn summary_ast_grep_rewrite(v: &serde_json::Value) -> Option<String> {
    let lang = v.get("lang").and_then(|x| x.as_str()).unwrap_or("?");
    let p = v.get("pattern").and_then(|x| x.as_str()).unwrap_or("");
    let short = if p.chars().count() > 42 {
        format!("{}…", p.chars().take(42).collect::<String>())
    } else {
        p.to_string()
    };
    let dry = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(true);
    Some(format!(
        "ast-grep rewrite [{}] {}{}",
        lang,
        short,
        if dry { " (dry-run)" } else { "" }
    ))
}

pub(super) fn summary_git_diff(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("git diff ({})", mode))
    } else {
        Some(format!("git diff ({}): {}", mode, path.trim()))
    }
}

pub(super) fn summary_git_diff_stat(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("git diff --stat ({})", mode))
    } else {
        Some(format!("git diff --stat ({}): {}", mode, path.trim()))
    }
}

pub(super) fn summary_git_diff_names(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("git diff --name-only ({})", mode))
    } else {
        Some(format!("git diff --name-only ({}): {}", mode, path.trim()))
    }
}

pub(super) fn summary_create_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("create file: {}", path))
}

pub(super) fn summary_modify_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("full");
    if mode == "replace_lines" {
        let s = v.get("start_line").and_then(|x| x.as_u64());
        let e = v.get("end_line").and_then(|x| x.as_u64());
        Some(format!(
            "replace lines {}-{} in {}",
            s.unwrap_or(0),
            e.unwrap_or(0),
            path
        ))
    } else {
        Some(format!("modify file: {}", path))
    }
}

pub(super) fn summary_copy_file(v: &serde_json::Value) -> Option<String> {
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("copy file: {} → {}", from, to))
}

pub(super) fn summary_move_file(v: &serde_json::Value) -> Option<String> {
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("move file: {} → {}", from, to))
}

pub(super) fn summary_read_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let start = v.get("start_line").and_then(|x| x.as_u64());
    let end = v.get("end_line").and_then(|x| x.as_u64());
    let ml = v.get("max_lines").and_then(|x| x.as_u64());
    let enc = v
        .get("encoding")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let suffix = match (start, end, ml) {
        (Some(s), Some(e), _) => format!(" [{}-{}]", s, e),
        (Some(s), None, Some(m)) => format!(" [{}~ max_lines={}]", s, m),
        (Some(s), None, None) => format!(" [{}~]", s),
        (None, Some(e), _) => format!(" [1-{}]", e),
        (None, None, Some(m)) => format!(" [chunk max_lines={}]", m),
        (None, None, None) => String::new(),
    };
    let enc_s = enc.map(|e| format!(" enc={}", e)).unwrap_or_default();
    Some(format!("read file: {}{}{}", path, suffix, enc_s))
}

pub(super) fn summary_read_dir(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    let p = if path.is_empty() { "." } else { path };
    Some(format!("read dir: {}", p))
}

pub(super) fn summary_web_search(v: &serde_json::Value) -> Option<String> {
    let q = v.get("query")?.as_str()?.trim();
    Some(format!("web search: {}", q))
}

pub(super) fn summary_http_fetch(v: &serde_json::Value) -> Option<String> {
    let u = v.get("url")?.as_str()?.trim();
    let m = v
        .get("method")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("GET");
    Some(format!("HTTP {} {}", m.to_ascii_uppercase(), u))
}

pub(super) fn summary_http_request(v: &serde_json::Value) -> Option<String> {
    let u = v.get("url")?.as_str()?.trim();
    let m = v.get("method")?.as_str()?.trim();
    Some(format!("HTTP {} {}", m.to_ascii_uppercase(), u))
}

pub(super) fn summary_glob_files(v: &serde_json::Value) -> Option<String> {
    let pat = v.get("pattern")?.as_str()?.trim();
    let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    Some(format!(
        "glob {} @ {}",
        pat,
        if root.is_empty() { "." } else { root }
    ))
}

pub(super) fn summary_markdown_check_links(v: &serde_json::Value) -> Option<String> {
    let roots = v
        .get("roots")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "README.md, docs".to_string());
    Some(format!("markdown link check: {}", roots))
}

pub(super) fn summary_structured_validate(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("structured validate: {}", path))
}

pub(super) fn summary_structured_query(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let q = v.get("query")?.as_str()?.trim();
    Some(format!("structured query: {} [{}]", path, q))
}

pub(super) fn summary_structured_diff(v: &serde_json::Value) -> Option<String> {
    let a = v.get("path_a")?.as_str()?.trim();
    let b = v.get("path_b")?.as_str()?.trim();
    Some(format!("structured diff: {} vs {}", a, b))
}

pub(super) fn summary_structured_patch(v: &serde_json::Value) -> Option<String> {
    let p = v.get("path")?.as_str()?.trim();
    let q = v.get("query")?.as_str()?.trim();
    let a = v.get("action").and_then(|x| x.as_str()).unwrap_or("set");
    Some(format!("structured patch: {} [{} @ {}]", p, a, q))
}

pub(super) fn summary_list_tree(v: &serde_json::Value) -> Option<String> {
    let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    Some(format!(
        "list tree: {}",
        if root.is_empty() { "." } else { root }
    ))
}

pub(super) fn summary_file_exists(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("file exists: {}", path))
}

pub(super) fn summary_read_binary_meta(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("binary metadata: {}", path))
}

pub(super) fn summary_hash_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let algo = v
        .get("algorithm")
        .and_then(|x| x.as_str())
        .unwrap_or("sha256");
    Some(format!("file hash {}: {}", algo, path))
}

pub(super) fn summary_extract_in_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let pattern = v.get("pattern")?.as_str()?.trim();
    let enc = v
        .get("encoding")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let enc_s = enc.map(|e| format!(" enc={}", e)).unwrap_or_default();
    Some(format!("extract in file: {} / {}{}", path, pattern, enc_s))
}

pub(super) fn summary_apply_patch(v: &serde_json::Value) -> Option<String> {
    let patch = v.get("patch")?.as_str()?;
    let files = patch
        .lines()
        .filter_map(|line| line.strip_prefix("+++ "))
        .map(|s| s.split_whitespace().next().unwrap_or(""))
        .filter(|s| !s.is_empty() && *s != "/dev/null")
        .map(|s| {
            s.trim_start_matches("b/")
                .trim_start_matches("a/")
                .to_string()
        })
        .collect::<Vec<_>>();
    if files.is_empty() {
        Some("apply patch".to_string())
    } else {
        Some(format!("apply patch: {}", files.join(", ")))
    }
}

pub(super) fn summary_run_executable(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let args = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(format!("run executable: {}", path))
    } else {
        Some(format!("run executable: {} {}", path, args))
    }
}

pub(super) fn summary_package_query(v: &serde_json::Value) -> Option<String> {
    let pkg = v.get("package")?.as_str()?.trim();
    let mgr = v
        .get("manager")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    Some(format!("package query: {} ({})", pkg, mgr))
}

pub(super) fn summary_find_symbol(v: &serde_json::Value) -> Option<String> {
    let symbol = v.get("symbol")?.as_str()?.trim();
    Some(format!("find symbol: {}", symbol))
}

pub(super) fn summary_find_references(v: &serde_json::Value) -> Option<String> {
    let symbol = v.get("symbol")?.as_str()?.trim();
    Some(format!("find references: {}", symbol))
}

pub(super) fn summary_rust_file_outline(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("Rust outline: {}", path))
}

pub(super) fn summary_format_check_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("format check: {}", path))
}

pub(super) fn summary_convert_units(v: &serde_json::Value) -> Option<String> {
    let cat = v.get("category")?.as_str()?.trim();
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("convert units: {} ({} → {})", cat, from, to))
}

// ── Git write summaries ──────────────────────────────────────────

pub(super) fn summary_git_checkout(v: &serde_json::Value) -> Option<String> {
    let target = v.get("target")?.as_str()?.trim();
    let create = v.get("create").and_then(|x| x.as_bool()).unwrap_or(false);
    if create {
        Some(format!("git checkout -b {}", target))
    } else {
        Some(format!("git checkout {}", target))
    }
}

pub(super) fn summary_git_branch_create(v: &serde_json::Value) -> Option<String> {
    let name = v.get("name")?.as_str()?.trim();
    Some(format!("git branch create: {}", name))
}

pub(super) fn summary_git_branch_delete(v: &serde_json::Value) -> Option<String> {
    let name = v.get("name")?.as_str()?.trim();
    Some(format!("git branch delete: {}", name))
}

pub(super) fn summary_git_push(v: &serde_json::Value) -> Option<String> {
    let remote = v.get("remote").and_then(|x| x.as_str()).unwrap_or("origin");
    let branch = v.get("branch").and_then(|x| x.as_str()).unwrap_or("");
    if branch.is_empty() {
        Some(format!("git push {}", remote))
    } else {
        Some(format!("git push {} {}", remote, branch))
    }
}

pub(super) fn summary_git_merge(v: &serde_json::Value) -> Option<String> {
    let branch = v.get("branch")?.as_str()?.trim();
    Some(format!("git merge {}", branch))
}

pub(super) fn summary_git_rebase(v: &serde_json::Value) -> Option<String> {
    if v.get("abort").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Some("git rebase --abort".to_string());
    }
    if v.get("continue").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Some("git rebase --continue".to_string());
    }
    let onto = v.get("onto").and_then(|x| x.as_str()).unwrap_or("?");
    Some(format!("git rebase onto {}", onto))
}

pub(super) fn summary_git_stash(v: &serde_json::Value) -> Option<String> {
    let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("push");
    Some(format!("git stash {}", action))
}

pub(super) fn summary_git_tag(v: &serde_json::Value) -> Option<String> {
    let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("list");
    match action {
        "create" => {
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            Some(format!("git tag create: {}", name))
        }
        "delete" => {
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            Some(format!("git tag delete: {}", name))
        }
        _ => Some("git tag list".to_string()),
    }
}

pub(super) fn summary_git_reset(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("mixed");
    let target = v.get("target").and_then(|x| x.as_str()).unwrap_or("HEAD");
    Some(format!("git reset --{} {}", mode, target))
}

pub(super) fn summary_git_revert(v: &serde_json::Value) -> Option<String> {
    if v.get("abort").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Some("git revert --abort".to_string());
    }
    let commit = v.get("commit").and_then(|x| x.as_str()).unwrap_or("?");
    Some(format!("git revert {}", commit))
}

// ── Node.js / npm ───────────────────────────────────────────

pub(super) fn summary_npm_run(v: &serde_json::Value) -> Option<String> {
    let script = v.get("script")?.as_str()?.trim();
    Some(format!("npm run {}", script))
}

pub(super) fn summary_npx_run(v: &serde_json::Value) -> Option<String> {
    let pkg = v.get("package")?.as_str()?.trim();
    Some(format!("npx {}", pkg))
}

// ── Process & ports ─────────────────────────────────────────

pub(super) fn summary_port_check(v: &serde_json::Value) -> Option<String> {
    let port = v.get("port")?.as_u64()?;
    Some(format!("port check: {}", port))
}

pub(super) fn summary_process_list(v: &serde_json::Value) -> Option<String> {
    let filter = v.get("filter").and_then(|x| x.as_str()).unwrap_or("");
    if filter.is_empty() {
        Some("list processes".to_string())
    } else {
        Some(format!("list processes (filter: {})", filter))
    }
}

// ── Code metrics & analysis ──────────────────────────────────

pub(super) fn summary_code_stats(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".");
    Some(format!("code stats: {}", path))
}

pub(super) fn summary_dependency_graph(v: &serde_json::Value) -> Option<String> {
    let format = v
        .get("format")
        .and_then(|x| x.as_str())
        .unwrap_or("mermaid");
    let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("auto");
    Some(format!("dependency graph ({}/{})", kind, format))
}

pub(super) fn summary_coverage_report(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.is_empty() {
        Some("coverage report (auto-detect)".to_string())
    } else {
        Some(format!("coverage report: {}", path))
    }
}

// ── File tools ───────────────────────────────────────────────

pub(super) fn summary_delete_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("delete file: {}", path))
}

pub(super) fn summary_delete_dir(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let recursive = v
        .get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    if recursive {
        Some(format!("delete directory (recursive): {}", path))
    } else {
        Some(format!("delete directory: {}", path))
    }
}

pub(super) fn summary_append_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("append to file: {}", path))
}

pub(super) fn summary_create_dir(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("create directory: {}", path))
}

pub(super) fn summary_search_replace(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let search = v.get("search")?.as_str()?;
    let dry = v.get("dry_run").and_then(|d| d.as_bool()).unwrap_or(true);
    let short = if search.chars().count() > 30 {
        format!("{}…", search.chars().take(30).collect::<String>())
    } else {
        search.to_string()
    };
    Some(format!(
        "search-replace{}: {} / \"{}\"",
        if dry { " (preview)" } else { "" },
        path,
        short
    ))
}

pub(super) fn summary_chmod_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let mode = v.get("mode")?.as_str()?.trim();
    Some(format!("chmod: {} → {}", path, mode))
}

pub(super) fn summary_symlink_info(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("symlink info: {}", path))
}
