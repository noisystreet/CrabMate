/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub(crate) fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    match name {
        "run_command" => {
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
            let s = if args.is_empty() {
                format!("执行命令：{}", cmd)
            } else {
                format!("执行命令：{} {}", cmd, args)
            };
            Some(s)
        }
        "cargo_check" => Some("运行 cargo check".to_string()),
        "cargo_test" => Some("运行 cargo test".to_string()),
        "cargo_clippy" => Some("运行 cargo clippy".to_string()),
        "cargo_metadata" => Some("读取 cargo metadata".to_string()),
        "cargo_tree" => Some("查看 cargo 依赖树".to_string()),
        "cargo_clean" => Some("运行 cargo clean".to_string()),
        "cargo_doc" => Some("生成 cargo 文档".to_string()),
        "cargo_run" => Some("运行 cargo run".to_string()),
        "cargo_nextest" => Some("运行 cargo nextest".to_string()),
        "cargo_fmt_check" => Some("运行 cargo fmt --check".to_string()),
        "cargo_outdated" => Some("运行 cargo outdated".to_string()),
        "cargo_machete" => Some("运行 cargo machete".to_string()),
        "cargo_udeps" => Some("运行 cargo udeps".to_string()),
        "cargo_publish_dry_run" => Some("cargo publish --dry-run".to_string()),
        "rust_compiler_json" => Some("cargo check JSON 诊断".to_string()),
        "rust_analyzer_goto_definition" => {
            let path = v.get("path")?.as_str()?.trim();
            let line = v.get("line").and_then(|x| x.as_u64());
            Some(format!("RA 跳转定义：{}:{}", path, line.unwrap_or(0)))
        }
        "rust_analyzer_find_references" => {
            let path = v.get("path")?.as_str()?.trim();
            let line = v.get("line").and_then(|x| x.as_u64());
            Some(format!("RA 查找引用：{}:{}", path, line.unwrap_or(0)))
        }
        "cargo_fix" => Some("运行 cargo fix（受控写入）".to_string()),
        "rust_test_one" => Some("运行单个 Rust 测试".to_string()),
        "frontend_lint" => Some("运行前端 lint".to_string()),
        "frontend_build" => Some("运行前端 build".to_string()),
        "frontend_test" => Some("运行前端 test".to_string()),
        "ruff_check" => Some("运行 ruff check".to_string()),
        "pytest_run" => Some("运行 python3 -m pytest".to_string()),
        "mypy_check" => Some("运行 mypy".to_string()),
        "python_install_editable" => {
            let b = v.get("backend").and_then(|x| x.as_str()).unwrap_or("?");
            Some(format!("Python 可编辑安装（{}）", b))
        }
        "uv_sync" => Some("运行 uv sync".to_string()),
        "uv_run" => {
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
        "pre_commit_run" => {
            let hook = v.get("hook").and_then(|x| x.as_str()).unwrap_or("");
            if hook.is_empty() {
                Some("运行 pre-commit".to_string())
            } else {
                Some(format!("pre-commit：{}", hook))
            }
        }
        "typos_check" => Some("typos 拼写检查".to_string()),
        "codespell_check" => Some("codespell 拼写检查".to_string()),
        "ast_grep_run" => {
            let lang = v.get("lang").and_then(|x| x.as_str()).unwrap_or("?");
            let p = v.get("pattern").and_then(|x| x.as_str()).unwrap_or("");
            let short = if p.chars().count() > 48 {
                format!("{}…", p.chars().take(48).collect::<String>())
            } else {
                p.to_string()
            };
            Some(format!("ast-grep [{}] {}", lang, short))
        }
        "ast_grep_rewrite" => {
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
        "cargo_audit" => Some("运行 cargo audit".to_string()),
        "cargo_deny" => Some("运行 cargo deny".to_string()),
        "ci_pipeline_local" => Some("运行本地 CI 流水线".to_string()),
        "release_ready_check" => Some("运行发布前一键检查".to_string()),
        "workflow_execute" => Some("执行 DAG 工作流".to_string()),
        "rust_backtrace_analyze" => Some("分析 Rust backtrace".to_string()),
        "diagnostic_summary" => Some("环境/工具链诊断摘要（脱敏）".to_string()),
        "changelog_draft" => Some("生成变更日志 Markdown 草稿".to_string()),
        "license_notice" => Some("依赖许可证摘要表（cargo metadata）".to_string()),
        "git_status" => Some("查看 Git 状态".to_string()),
        "git_clean_check" => Some("检查 Git 工作区是否干净".to_string()),
        "git_diff" => {
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.trim().is_empty() {
                Some(format!("查看 Git diff（{}）", mode))
            } else {
                Some(format!("查看 Git diff（{}）：{}", mode, path.trim()))
            }
        }
        "git_diff_stat" => {
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.trim().is_empty() {
                Some(format!("查看 Git diff 统计（{}）", mode))
            } else {
                Some(format!("查看 Git diff 统计（{}）：{}", mode, path.trim()))
            }
        }
        "git_diff_names" => {
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
        "git_log" => Some("查看 Git 提交历史".to_string()),
        "git_show" => Some("查看 Git 提交详情".to_string()),
        "git_diff_base" => Some("查看 base...HEAD 差异".to_string()),
        "git_blame" => Some("查看 Git blame".to_string()),
        "git_file_history" => Some("查看文件 Git 历史".to_string()),
        "git_branch_list" => Some("查看分支列表".to_string()),
        "git_remote_status" => Some("查看远程跟踪状态".to_string()),
        "git_stage_files" => Some("暂存文件".to_string()),
        "git_commit" => Some("提交变更".to_string()),
        "git_fetch" => Some("拉取远程更新".to_string()),
        "git_remote_list" => Some("查看远程仓库".to_string()),
        "git_remote_set_url" => Some("设置远程 URL".to_string()),
        "git_apply" => Some("应用 Git 补丁".to_string()),
        "git_clone" => Some("克隆仓库".to_string()),
        "create_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("新建文件：{}", path))
        }
        "modify_file" => {
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
        "copy_file" => {
            let from = v.get("from")?.as_str()?.trim();
            let to = v.get("to")?.as_str()?.trim();
            Some(format!("复制文件：{} → {}", from, to))
        }
        "move_file" => {
            let from = v.get("from")?.as_str()?.trim();
            let to = v.get("to")?.as_str()?.trim();
            Some(format!("移动文件：{} → {}", from, to))
        }
        "read_file" => {
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
        "read_dir" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            if path.is_empty() {
                Some("读取目录".to_string())
            } else {
                Some(format!("读取目录：{}", path))
            }
        }
        "web_search" => {
            let q = v.get("query")?.as_str()?.trim();
            Some(format!("联网搜索：{}", q))
        }
        "http_fetch" => {
            let u = v.get("url")?.as_str()?.trim();
            let m = v
                .get("method")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("GET");
            Some(format!("HTTP {}：{}", m.to_ascii_uppercase(), u))
        }
        "http_request" => {
            let u = v.get("url")?.as_str()?.trim();
            let m = v.get("method")?.as_str()?.trim();
            Some(format!("HTTP {}：{}", m.to_ascii_uppercase(), u))
        }
        "glob_files" => {
            let pat = v.get("pattern")?.as_str()?.trim();
            let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            Some(format!(
                "glob 匹配：{} @ {}",
                pat,
                if root.is_empty() { "." } else { root }
            ))
        }
        "markdown_check_links" => {
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
        "structured_validate" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("结构化校验：{}", path))
        }
        "structured_query" => {
            let path = v.get("path")?.as_str()?.trim();
            let q = v.get("query")?.as_str()?.trim();
            Some(format!("结构化查询：{} [{}]", path, q))
        }
        "structured_diff" => {
            let a = v.get("path_a")?.as_str()?.trim();
            let b = v.get("path_b")?.as_str()?.trim();
            Some(format!("结构化 diff：{} vs {}", a, b))
        }
        "structured_patch" => {
            let p = v.get("path")?.as_str()?.trim();
            let q = v.get("query")?.as_str()?.trim();
            let a = v.get("action").and_then(|x| x.as_str()).unwrap_or("set");
            Some(format!("结构化补丁：{} [{} @ {}]", p, a, q))
        }
        "list_tree" => {
            let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            Some(format!(
                "递归列目录：{}",
                if root.is_empty() { "." } else { root }
            ))
        }
        "file_exists" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("检查是否存在：{}", path))
        }
        "read_binary_meta" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("二进制元数据：{}", path))
        }
        "hash_file" => {
            let path = v.get("path")?.as_str()?.trim();
            let algo = v
                .get("algorithm")
                .and_then(|x| x.as_str())
                .unwrap_or("sha256");
            Some(format!("文件哈希 {}：{}", algo, path))
        }
        "extract_in_file" => {
            let path = v.get("path")?.as_str()?.trim();
            let pattern = v.get("pattern")?.as_str()?.trim();
            Some(format!("从文件提取匹配：{} / {}", path, pattern))
        }
        "apply_patch" => {
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
        "run_executable" => {
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
            let s = if args.is_empty() {
                format!("运行可执行：{}", path)
            } else {
                format!("运行可执行：{} {}", path, args)
            };
            Some(s)
        }
        "package_query" => {
            let pkg = v.get("package")?.as_str()?.trim();
            let mgr = v
                .get("manager")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("auto");
            Some(format!("查询系统包：{}（{}）", pkg, mgr))
        }
        "find_symbol" => {
            let symbol = v.get("symbol")?.as_str()?.trim();
            Some(format!("查找符号：{}", symbol))
        }
        "find_references" => {
            let symbol = v.get("symbol")?.as_str()?.trim();
            Some(format!("查找引用：{}", symbol))
        }
        "rust_file_outline" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("Rust 大纲：{}", path))
        }
        "format_check_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("格式检查：{}", path))
        }
        "quality_workspace" => Some("工作区质量检查".to_string()),
        "convert_units" => {
            let cat = v.get("category")?.as_str()?.trim();
            let from = v.get("from")?.as_str()?.trim();
            let to = v.get("to")?.as_str()?.trim();
            Some(format!("单位换算：{}（{} → {}）", cat, from, to))
        }
        _ => None,
    }
}
