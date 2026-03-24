/// 动态摘要辅助函数：由 `ToolSpec::summary` 的 `Dynamic` 变体引用。
/// 每个函数从解析后的 `serde_json::Value` 中提取关键参数，生成一行简短中文摘要。
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
        Some(format!("执行命令：{}", cmd))
    } else {
        Some(format!("执行命令：{} {}", cmd, args))
    }
}

pub(super) fn summary_rust_analyzer_goto_definition(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let line = v.get("line").and_then(|x| x.as_u64());
    Some(format!("RA 跳转定义：{}:{}", path, line.unwrap_or(0)))
}

pub(super) fn summary_rust_analyzer_find_references(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let line = v.get("line").and_then(|x| x.as_u64());
    Some(format!("RA 查找引用：{}:{}", path, line.unwrap_or(0)))
}

pub(super) fn summary_python_install_editable(v: &serde_json::Value) -> Option<String> {
    let b = v.get("backend").and_then(|x| x.as_str()).unwrap_or("?");
    Some(format!("Python 可编辑安装（{}）", b))
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

pub(super) fn summary_pre_commit_run(v: &serde_json::Value) -> Option<String> {
    let hook = v.get("hook").and_then(|x| x.as_str()).unwrap_or("");
    if hook.is_empty() {
        Some("运行 pre-commit".to_string())
    } else {
        Some(format!("pre-commit：{}", hook))
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
        if dry { "（dry-run）" } else { "" }
    ))
}

pub(super) fn summary_git_diff(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("查看 Git diff（{}）", mode))
    } else {
        Some(format!("查看 Git diff（{}）：{}", mode, path.trim()))
    }
}

pub(super) fn summary_git_diff_stat(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("查看 Git diff 统计（{}）", mode))
    } else {
        Some(format!("查看 Git diff 统计（{}）：{}", mode, path.trim()))
    }
}

pub(super) fn summary_git_diff_names(v: &serde_json::Value) -> Option<String> {
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    if path.trim().is_empty() {
        Some(format!("查看 Git diff 变更文件名（{}）", mode))
    } else {
        Some(format!(
            "查看 Git diff 变更文件名（{}）：{}",
            mode,
            path.trim()
        ))
    }
}

pub(super) fn summary_create_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("新建文件：{}", path))
}

pub(super) fn summary_modify_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("full");
    if mode == "replace_lines" {
        let s = v.get("start_line").and_then(|x| x.as_u64());
        let e = v.get("end_line").and_then(|x| x.as_u64());
        Some(format!(
            "修改文件（行替换 {}-{}）：{}",
            s.unwrap_or(0),
            e.unwrap_or(0),
            path
        ))
    } else {
        Some(format!("修改文件：{}", path))
    }
}

pub(super) fn summary_copy_file(v: &serde_json::Value) -> Option<String> {
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("复制文件：{} → {}", from, to))
}

pub(super) fn summary_move_file(v: &serde_json::Value) -> Option<String> {
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("移动文件：{} → {}", from, to))
}

pub(super) fn summary_read_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let start = v.get("start_line").and_then(|x| x.as_u64());
    let end = v.get("end_line").and_then(|x| x.as_u64());
    let ml = v.get("max_lines").and_then(|x| x.as_u64());
    let suffix = match (start, end, ml) {
        (Some(s), Some(e), _) => format!(" [{}-{}]", s, e),
        (Some(s), None, Some(m)) => format!(" [{}~ max_lines={}]", s, m),
        (Some(s), None, None) => format!(" [{}~]", s),
        (None, Some(e), _) => format!(" [1-{}]", e),
        (None, None, Some(m)) => format!(" [分段 max_lines={}]", m),
        (None, None, None) => String::new(),
    };
    Some(format!("读取文件：{}{}", path, suffix))
}

pub(super) fn summary_read_dir(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    if path.is_empty() {
        Some("读取目录".to_string())
    } else {
        Some(format!("读取目录：{}", path))
    }
}

pub(super) fn summary_web_search(v: &serde_json::Value) -> Option<String> {
    let q = v.get("query")?.as_str()?.trim();
    Some(format!("联网搜索：{}", q))
}

pub(super) fn summary_http_fetch(v: &serde_json::Value) -> Option<String> {
    let u = v.get("url")?.as_str()?.trim();
    let m = v
        .get("method")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("GET");
    Some(format!("HTTP {}：{}", m.to_ascii_uppercase(), u))
}

pub(super) fn summary_http_request(v: &serde_json::Value) -> Option<String> {
    let u = v.get("url")?.as_str()?.trim();
    let m = v.get("method")?.as_str()?.trim();
    Some(format!("HTTP {}：{}", m.to_ascii_uppercase(), u))
}

pub(super) fn summary_glob_files(v: &serde_json::Value) -> Option<String> {
    let pat = v.get("pattern")?.as_str()?.trim();
    let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    Some(format!(
        "glob 匹配：{} @ {}",
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
    Some(format!("Markdown 死链检查：{}", roots))
}

pub(super) fn summary_structured_validate(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("结构化校验：{}", path))
}

pub(super) fn summary_structured_query(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let q = v.get("query")?.as_str()?.trim();
    Some(format!("结构化查询：{} [{}]", path, q))
}

pub(super) fn summary_structured_diff(v: &serde_json::Value) -> Option<String> {
    let a = v.get("path_a")?.as_str()?.trim();
    let b = v.get("path_b")?.as_str()?.trim();
    Some(format!("结构化 diff：{} vs {}", a, b))
}

pub(super) fn summary_structured_patch(v: &serde_json::Value) -> Option<String> {
    let p = v.get("path")?.as_str()?.trim();
    let q = v.get("query")?.as_str()?.trim();
    let a = v.get("action").and_then(|x| x.as_str()).unwrap_or("set");
    Some(format!("结构化补丁：{} [{} @ {}]", p, a, q))
}

pub(super) fn summary_list_tree(v: &serde_json::Value) -> Option<String> {
    let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
    Some(format!(
        "递归列目录：{}",
        if root.is_empty() { "." } else { root }
    ))
}

pub(super) fn summary_file_exists(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("检查是否存在：{}", path))
}

pub(super) fn summary_read_binary_meta(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("二进制元数据：{}", path))
}

pub(super) fn summary_hash_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let algo = v
        .get("algorithm")
        .and_then(|x| x.as_str())
        .unwrap_or("sha256");
    Some(format!("文件哈希 {}：{}", algo, path))
}

pub(super) fn summary_extract_in_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    let pattern = v.get("pattern")?.as_str()?.trim();
    Some(format!("从文件提取匹配：{} / {}", path, pattern))
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
        Some("应用补丁".to_string())
    } else {
        Some(format!("应用补丁：{}", files.join(", ")))
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
        Some(format!("运行可执行：{}", path))
    } else {
        Some(format!("运行可执行：{} {}", path, args))
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
    Some(format!("查询系统包：{}（{}）", pkg, mgr))
}

pub(super) fn summary_find_symbol(v: &serde_json::Value) -> Option<String> {
    let symbol = v.get("symbol")?.as_str()?.trim();
    Some(format!("查找符号：{}", symbol))
}

pub(super) fn summary_find_references(v: &serde_json::Value) -> Option<String> {
    let symbol = v.get("symbol")?.as_str()?.trim();
    Some(format!("查找引用：{}", symbol))
}

pub(super) fn summary_rust_file_outline(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("Rust 大纲：{}", path))
}

pub(super) fn summary_format_check_file(v: &serde_json::Value) -> Option<String> {
    let path = v.get("path")?.as_str()?.trim();
    Some(format!("格式检查：{}", path))
}

pub(super) fn summary_convert_units(v: &serde_json::Value) -> Option<String> {
    let cat = v.get("category")?.as_str()?.trim();
    let from = v.get("from")?.as_str()?.trim();
    let to = v.get("to")?.as_str()?.trim();
    Some(format!("单位换算：{}（{} → {}）", cat, from, to))
}
