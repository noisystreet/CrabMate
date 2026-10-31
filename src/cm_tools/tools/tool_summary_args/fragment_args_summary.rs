// ── #1157：摘要与 Args 同源（`ToolSummaryKind::Dynamic` 的迁移目标形态）──────────
//
// 每条 `impl` 紧贴对应 Args 的 schema 定义（`tool_param_types`）阅读：字段名直接取自
// JSON，不再单独声明一份字段子集结构。于是「Args 改了、摘要没跟上」不会再以
// 「编译通过但摘要错位」的形态发生。
//
// 输出文本必须与迁移前逐字一致——`fixtures/tool_registry_golden.json` 的
// `dynamic_summary_samples` 钉住了这一点。

impl ToolArgsSummary for super::tool_param_types::ReadFileArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        let suffix = match arg_u64(args, "anchor_line") {
            Some(anchor) => format!(
                " [@L{} ±{}]",
                anchor,
                arg_u64(args, "context_lines").unwrap_or(120)
            ),
            None => read_file_range_suffix(
                arg_u64(args, "start_line"),
                arg_u64(args, "end_line"),
                arg_u64(args, "max_lines"),
            ),
        };
        let enc = arg_str(args, "encoding")
            .map(|e| format!(" enc={e}"))
            .unwrap_or_default();
        Some(format!("read file: {path}{suffix}{enc}"))
    }
}

/// `read_file` 的行区间后缀；与迁移前 `ReadFileSummaryArgs` 的分支逐字一致。
fn read_file_range_suffix(
    start: Option<u64>,
    end: Option<u64>,
    max_lines: Option<u64>,
) -> String {
    match (start, end, max_lines) {
        (Some(s), Some(e), _) => {
            let (lo, hi) = if e < s { (e, s) } else { (s, e) };
            format!(" [{lo}-{hi}]")
        }
        (Some(s), None, Some(m)) => format!(" [{s}~ max_lines={m}]"),
        (Some(s), None, None) => format!(" [{s}~]"),
        (None, Some(e), _) => format!(" [1-{e}]"),
        (None, None, Some(m)) => format!(" [chunk max_lines={m}]"),
        (None, None, None) => String::new(),
    }
}

impl ToolArgsSummary for super::tool_param_types::FileWriteArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        Some(format!("create file: {path}"))
    }
}

// ── P2 批：代码度量 / 后台任务的 5 个小摘要 ────────────────────────────────

impl ToolArgsSummary for super::tool_param_types::CodeStatsArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        // 旧结构体是 `Option<String>` + `unwrap_or(".")`：缺失回落 `.`，显式空白串则 trim
        // 成空串而**不**回落——用 `arg_str_raw` 保持这条边界，`arg_str` 会把它当「没给」。
        let path = arg_str_raw(args, "path").unwrap_or(".").trim();
        Some(format!("code stats: {path}"))
    }
}

impl ToolArgsSummary for super::tool_param_types::DependencyGraphArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let format = arg_str_raw(args, "format").unwrap_or("mermaid");
        let kind = arg_str_raw(args, "kind").unwrap_or("auto");
        Some(format!("dependency graph ({kind}/{format})"))
    }
}

impl ToolArgsSummary for super::tool_param_types::CoverageReportArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str_raw(args, "path").unwrap_or("").trim();
        if path.is_empty() {
            Some("coverage report (auto-detect)".to_string())
        } else {
            Some(format!("coverage report: {path}"))
        }
    }
}

impl ToolArgsSummary for super::tool_param_types::DeleteDirArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        Some(if arg_bool(args, "recursive").unwrap_or(false) {
            format!("delete directory (recursive): {path}")
        } else {
            format!("delete directory: {path}")
        })
    }
}

impl ToolArgsSummary for super::tool_param_types::BackgroundJobCancelArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        match arg_str(args, "tool_job_id") {
            Some(id) => Some(format!("cancel background job: {id}")),
            None => Some("cancel background job".to_string()),
        }
    }
}


/// 摘要里的「前 N 字符 + …」截断。原先 `gh issue create` / `gh api` / `gh pr` /
/// `search_replace` 各写一遍同样的逻辑，迁移时收敛到这里。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}
impl ToolArgsSummary for super::tool_param_types::ReadBinaryMetaArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("binary metadata: {}", arg_str(args, "path")?))
    }
}

impl ToolArgsSummary for super::tool_param_types::AppendFileArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("append to file: {}", arg_str(args, "path")?))
    }
}

impl ToolArgsSummary for super::tool_param_types::CreateDirArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("create directory: {}", arg_str(args, "path")?))
    }
}

impl ToolArgsSummary for super::tool_param_types::DeleteFilesArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        // 旧结构体是 `paths: Vec<String>`：字段缺失或不是数组都等于「没给」。
        let paths = args.get("paths")?.as_array()?;
        let total = paths.len();
        if total == 0 {
            return None;
        }
        let shown: Vec<&str> = paths.iter().take(3).filter_map(serde_json::Value::as_str).collect();
        let mut line = format!("delete files: {}", shown.join(", "));
        if total > 3 {
            line.push_str(&format!(" (+{} more)", total - 3));
        }
        Some(line)
    }
}

impl ToolArgsSummary for super::tool_param_types::GhReleaseCreateArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let tag = arg_str(args, "tag")?;
        let draft = arg_bool(args, "draft") == Some(true);
        Some(format!(
            "gh release create {tag}{}",
            if draft { " (draft)" } else { "" }
        ))
    }
}

impl ToolArgsSummary for super::tool_param_types::GhIssueCreateArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let title = arg_str(args, "title")?;
        Some(format!(
            "gh issue create{}: {}",
            gh_repo_suffix(arg_str(args, "repo")),
            truncate_chars(title, 40)
        ))
    }
}

impl ToolArgsSummary for super::tool_param_types::GhApiArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        // `method` 走 `arg_str_raw`：旧实现是 `unwrap_or("GET").trim()`，显式空白串会得到
        // 空 method 而不是回落 "GET"，这条边界要保持。
        let method = arg_str_raw(args, "method")
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        Some(format!("gh api {method} {}", truncate_chars(path, 40)))
    }
}


// ── P2 批：后台任务族（含 `AsyncOnly` 的 run_command / terminal_session）─────────

impl ToolArgsSummary for super::tool_param_types::RunCommandArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        // `command` 既可能是字符串也可能是数组（模型两种都发），所以不走 `arg_str`，
        // 直接看原始 JSON；数组会被拼成一整条命令行。
        let mut tokens = command_display_tokens(args.get("command")?);
        if tokens.is_empty() {
            return None;
        }
        let no_args = serde_json::Value::Null;
        tokens.extend(run_command_arg_display_tokens(
            args.get("args").unwrap_or(&no_args),
        ));
        Some(tokens.join(" "))
    }
}

impl ToolArgsSummary for super::tool_param_types::TerminalSessionArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action_raw = arg_str_raw(args, "action").unwrap_or("").trim();
        let action = action_raw.to_ascii_lowercase();
        // close/resize/signal/interrupt 走**不 trim** 的 session_id：旧实现
        // `unwrap_or("?")` 对空白串是原样输出，这里保持同样的边界。
        let sid_raw = args.get("session_id").and_then(serde_json::Value::as_str);
        let sid_or_q = sid_raw.unwrap_or("?");
        Some(match action.as_str() {
            "list" => "terminal_session list".to_string(),
            "close" => format!("terminal_session close {sid_or_q}"),
            "resize" => format!("terminal_session resize {sid_or_q}"),
            "send_signal" => format!("terminal_session signal {sid_or_q}"),
            "interrupt" => format!("terminal_session interrupt {sid_or_q}"),
            "exec" => {
                let sid = sid_raw.unwrap_or("").trim();
                let cmd = args
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !sid.is_empty() {
                    format!("terminal_session exec sid={sid}")
                } else if !cmd.is_empty() {
                    let tail = args
                        .get("args")
                        .and_then(serde_json::Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(terminal_session_arg_display_token)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    if tail.is_empty() {
                        format!("terminal_session exec {cmd}")
                    } else {
                        format!("terminal_session exec {cmd} {tail}")
                    }
                } else {
                    "terminal_session exec".to_string()
                }
            }
            _ => format!("terminal_session {action_raw}"),
        })
    }
}

impl ToolArgsSummary for super::tool_param_types::BackgroundJobArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action = arg_str_raw(args, "action").unwrap_or("").trim();
        let id = args
            .get("tool_job_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        Some(match action {
            "status" => {
                if id.is_empty() {
                    "background job status".to_string()
                } else {
                    format!("background job status: {id}")
                }
            }
            "list" => match arg_u64(args, "limit") {
                Some(n) => format!("list background jobs (limit: {n})"),
                None => "list background jobs".to_string(),
            },
            "output" => {
                if id.is_empty() {
                    "background job output".to_string()
                } else {
                    format!("background job output: {id}")
                }
            }
            "" => "background job".to_string(),
            other => format!("background job: {other}"),
        })
    }
}

impl ToolArgsSummary for super::tool_param_types::BackgroundJobWaitArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let id = args
            .get("tool_job_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let until = arg_str_raw(args, "until").unwrap_or("exit").trim();
        let target = match until {
            "output_matches" => match arg_str(args, "output_matches") {
                Some(p) => format!("output matches `{p}`"),
                None => "output matches".to_string(),
            },
            "" | "exit" => "exit".to_string(),
            other => other.to_string(),
        };
        let base = if id.is_empty() {
            format!("wait background job until {target}")
        } else {
            format!("wait background job {id} until {target}")
        };
        Some(match arg_u64(args, "timeout_secs") {
            Some(secs) => format!("{base} (timeout: {secs}s)"),
            None => base,
        })
    }
}


// ── P2 批：检索 / 结构化 / 受控 HTTP ────────────────────────────────────────

impl ToolArgsSummary for super::tool_param_types::SearchInFilesEnhancedArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let pat = truncate_chars(arg_str(args, "pattern")?, 40);
        Some(match arg_str(args, "path") {
            Some(p) => format!("search in files: {pat} @ {}", truncate_chars(p, 28)),
            None => format!("search in files: {pat}"),
        })
    }
}

impl ToolArgsSummary for super::tool_param_types::SearchReplaceArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        let short = truncate_chars(arg_str(args, "search")?, 30);
        // `max_replacements` / `replace_all` 的三种组合在摘要里必须可区分：默认是「唯一匹配
        // 守卫」，`(all)` 才是全量替换，`(first N)` 是显式限量。混成一句等于把「改多了」藏起来。
        let scope = match (
            arg_u64(args, "max_replacements"),
            arg_bool(args, "replace_all").unwrap_or(false),
        ) {
            (None, false) => "",
            (Some(0), _) | (None, true) => " (all)",
            (Some(n), _) => return Some(format!("search_replace (first {n}): {path} / \"{short}\"")),
        };
        let preview = arg_bool(args, "dry_run").unwrap_or(true);
        Some(format!(
            "search_replace{}{}: {} / \"{}\"",
            if preview { " (preview)" } else { "" },
            scope,
            path,
            short
        ))
    }
}

impl ToolArgsSummary for super::tool_param_types::StructuredArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path");
        match arg_str_raw(args, "action").unwrap_or("").trim() {
            "validate" => Some(format!("structured validate: {}", path?)),
            "query" => {
                let q = arg_str(args, "query")?;
                Some(format!("structured query: {} [{}]", path?, q))
            }
            "diff" => {
                let a = arg_str(args, "path_a")?;
                let b = arg_str(args, "path_b")?;
                Some(format!("structured diff: {} vs {}", a, b))
            }
            "patch" => {
                let q = arg_str(args, "query")?;
                let m = arg_str_raw(args, "mutation").unwrap_or("set").trim();
                Some(format!("structured patch: {} [{} @ {}]", path?, m, q))
            }
            _ => None,
        }
    }
}

/// `text_format` 归一化后属于「转纯文本」的取值才追加 `[html_text]` 标记。
/// `http_fetch` 与 `http_request` 原本各写一遍同样的判断，这里收敛成一个。
fn html_text_suffix(args: &serde_json::Value) -> &'static str {
    match arg_str(args, "text_format") {
        Some(tf) => {
            let norm = tf.to_ascii_lowercase().replace('-', "_");
            if matches!(norm.as_str(), "html_text" | "htmltext" | "text") {
                " [html_text]"
            } else {
                ""
            }
        }
        None => "",
    }
}

impl ToolArgsSummary for crate::cm_tools::tools::http_fetch::HttpFetchArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let url = arg_str(args, "url")?;
        let method = arg_str(args, "method").unwrap_or("GET").to_ascii_uppercase();
        Some(format!("HTTP {method} {url}{}", html_text_suffix(args)))
    }
}

impl ToolArgsSummary for crate::cm_tools::tools::http_fetch::HttpRequestArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let url = arg_str(args, "url")?;
        // 与 `http_fetch` 不同，这里的 `method` 是必填：缺了就不出摘要。
        let method = arg_str(args, "method")?.to_ascii_uppercase();
        Some(format!("HTTP {method} {url}{}", html_text_suffix(args)))
    }
}
// ── P2 批：文件 / 目录族的 6 个小摘要 ───────────────────────────────────────
//
// 这一族原先各有一个字段子集结构体（`CopyFileSummaryArgs` … `MarkdownCheckLinksSummaryArgs`），
// 字段名与 Args 完全一致、只少几个摘要用不到的键，于是直接按字段名取值即可。

/// 可选 `path` 的统一语义（`read_dir` / `list_tree` / `glob_files` 共用）：
/// 缺失、`null`、显式空白串都回落 `.`。**类型不是字符串**时返回 `None`——旧结构体是
/// `Option<String>`，反序列化失败等价于「没有摘要」，这条边界不因迁移而放宽。
fn path_or_dot(args: &serde_json::Value) -> Option<&str> {
    match args.get("path") {
        None | Some(serde_json::Value::Null) => Some("."),
        Some(serde_json::Value::String(raw)) => {
            let trimmed = raw.trim();
            Some(if trimmed.is_empty() { "." } else { trimmed })
        }
        Some(_) => None,
    }
}

/// `copy_file` / `move_file` 共用 `FileFromToOverwriteArgs`：字段完全一致、只有动词不同，
/// 所以摘要文本无法由 Args 类型单独决定（`specs` 里用 `summary: dynamic by <fn>`）。
fn from_to_line(args: &serde_json::Value, verb: &str) -> Option<String> {
    let from = arg_str(args, "from")?;
    let to = arg_str(args, "to")?;
    Some(format!("{verb}: {from} → {to}"))
}

pub(super) fn summary_copy_file(args: &serde_json::Value) -> Option<String> {
    from_to_line(args, "copy file")
}

pub(super) fn summary_move_file(args: &serde_json::Value) -> Option<String> {
    from_to_line(args, "move file")
}

impl ToolArgsSummary for super::tool_param_types::ReadDirEnhancedArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("read dir: {}", path_or_dot(args)?))
    }
}

impl ToolArgsSummary for super::tool_param_types::GlobFilesArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let pattern = arg_str(args, "pattern")?;
        Some(format!("glob {pattern} @ {}", path_or_dot(args)?))
    }
}

impl ToolArgsSummary for super::tool_param_types::ListTreeArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("list tree: {}", path_or_dot(args)?))
    }
}

impl ToolArgsSummary for super::tool_param_types::MarkdownCheckLinksArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let joined = arg_str_vec(args, "roots")?
            .into_iter()
            .map(str::trim)
            .filter(|root| !root.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        // 没有可用 roots 时旧实现回落内置默认清单，摘要里也照原样显示那两项。
        Some(format!(
            "markdown link check: {}",
            if joined.is_empty() {
                "README.md, docs"
            } else {
                joined.as_str()
            }
        ))
    }
}

#[cfg(test)]
mod args_summary_tests {
    use super::*;
    use serde_json::json;

    /// 迁移后最容易回退的一处：Args 带 `deny_unknown_fields`，但摘要必须对
    /// 「多带一个字段」的 arguments 仍然生成（旧 `*SummaryArgs` 的宽容语义）。
    #[test]
    fn read_file_summary_tolerates_unknown_keys() {
        let v = json!({"path": "src/lib.rs", "bogus": 1, "anchor_line": 7});
        assert_eq!(
            super::super::tool_param_types::ReadFileArgs::summary_line(&v).as_deref(),
            Some("read file: src/lib.rs [@L7 ±120]")
        );
    }

    #[test]
    fn read_file_summary_keeps_range_and_encoding_forms() {
        let v = json!({"path": "a.rs", "start_line": 9, "end_line": 3, "encoding": " gbk "});
        assert_eq!(
            super::super::tool_param_types::ReadFileArgs::summary_line(&v).as_deref(),
            Some("read file: a.rs [3-9] enc=gbk")
        );
    }

    /// 缺 `path` 时没有摘要（与旧结构体「反序列化失败 ⇒ 无摘要」一致）。
    #[test]
    fn create_file_summary_requires_path() {
        assert!(
            super::super::tool_param_types::FileWriteArgs::summary_line(&json!({"content": ""}))
                .is_none()
        );
        assert_eq!(
            super::super::tool_param_types::FileWriteArgs::summary_line(
                &json!({"path": " notes/a.md ", "content": ""})
            )
            .as_deref(),
            Some("create file: notes/a.md")
        );
    }
}

#[cfg(test)]
mod background_family_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::{BackgroundJobArgs, RunCommandArgs};
    use serde_json::json;

    // 下列案例从 `fragment_gh_archive.rs` / `fragment_git_files_tail.rs` 的旧测试
    // 平移而来：断言文本不变，只把入口从 `summarize_from_value::<*SummaryArgs>`
    // 换成 Args 上的 `summary_line`，确保迁移没有悄悄改掉输出。

    #[test]
    fn run_command_summary_joins_command_and_string_args() {
        let v = json!({ "command": "cargo", "args": ["test", "--all"] });
        assert_eq!(RunCommandArgs::summary_line(&v).expect("summary"), "cargo test --all");
    }

    #[test]
    fn run_command_summary_accepts_args_as_single_string() {
        let v = json!({ "command": "bash", "args": "-c echo hi" });
        let s = RunCommandArgs::summary_line(&v).expect("summary");
        assert!(s.starts_with("bash "), "{s}");
        assert!(s.contains("echo hi"), "{s}");
    }

    #[test]
    fn run_command_summary_quotes_args_with_spaces() {
        let v = json!({ "command": "bash", "args": ["-c", "echo hello world"] });
        assert_eq!(
            RunCommandArgs::summary_line(&v).expect("summary"),
            r#"bash -c "echo hello world""#
        );
    }

    #[test]
    fn background_job_status_summary_uses_job_id() {
        let v = json!({ "action": "status", "tool_job_id": " j1 " });
        assert_eq!(
            BackgroundJobArgs::summary_line(&v).expect("summary"),
            "background job status: j1"
        );
    }

    #[test]
    fn background_job_list_summary_reports_limit() {
        let v = json!({ "action": "list", "limit": 5 });
        assert_eq!(
            BackgroundJobArgs::summary_line(&v).expect("summary"),
            "list background jobs (limit: 5)"
        );
    }

    #[test]
    fn background_job_missing_action_falls_back_to_generic_line() {
        let v = json!({ "tool_job_id": "j1" });
        assert_eq!(
            BackgroundJobArgs::summary_line(&v).expect("summary"),
            "background job"
        );
    }
}

#[cfg(test)]
mod field_family_summary_tests {
    use super::*;
    use crate::cm_tools::tools::http_fetch::{HttpFetchArgs, HttpRequestArgs};
    use crate::cm_tools::tools::tool_param_types::{SearchReplaceArgs, StructuredArgs};
    use serde_json::json;

    // 这组钉的是「同一份入参下摘要必须能区分语义」的边界，快照样本覆盖不到。

    #[test]
    fn search_replace_default_is_unique_guard() {
        let v = json!({ "path": "a.rs", "search": "foo" });
        assert_eq!(
            SearchReplaceArgs::summary_line(&v).expect("summary"),
            "search_replace (preview): a.rs / \"foo\""
        );
    }

    #[test]
    fn search_replace_replace_all_and_first_n_are_distinguishable() {
        let all = json!({ "path": "a.rs", "search": "foo", "dry_run": false, "replace_all": true });
        assert_eq!(
            SearchReplaceArgs::summary_line(&all).expect("summary"),
            "search_replace (all): a.rs / \"foo\""
        );
        let zero = json!({ "path": "a.rs", "search": "foo", "dry_run": false, "max_replacements": 0 });
        assert_eq!(
            SearchReplaceArgs::summary_line(&zero).expect("summary"),
            "search_replace (all): a.rs / \"foo\""
        );
        let first3 = json!({ "path": "a.rs", "search": "foo", "max_replacements": 3 });
        assert_eq!(
            SearchReplaceArgs::summary_line(&first3).expect("summary"),
            "search_replace (first 3): a.rs / \"foo\""
        );
    }

    #[test]
    fn structured_patch_defaults_mutation_to_set() {
        let v = json!({ "action": "patch", "path": "c.toml", "query": "a.b" });
        assert_eq!(
            StructuredArgs::summary_line(&v).expect("summary"),
            "structured patch: c.toml [set @ a.b]"
        );
    }

    #[test]
    fn structured_unknown_action_has_no_summary() {
        let v = json!({ "action": "nope", "path": "c.toml" });
        assert!(StructuredArgs::summary_line(&v).is_none());
    }

    #[test]
    fn http_request_requires_method_but_fetch_defaults_to_get() {
        let req = json!({ "url": "https://example.com" });
        assert!(HttpRequestArgs::summary_line(&req).is_none());
        assert_eq!(
            HttpFetchArgs::summary_line(&req).expect("summary"),
            "HTTP GET https://example.com"
        );
    }

    #[test]
    fn html_text_format_variants_all_mark_the_suffix() {
        for tf in ["html_text", "htmltext", "text", "text-format-text"] {
            let v = json!({ "url": "https://example.com", "text_format": tf });
            let s = HttpFetchArgs::summary_line(&v).expect("summary");
            if tf == "text-format-text" {
                assert!(!s.contains("[html_text]"), "{tf} → {s}");
            } else {
                assert!(s.ends_with(" [html_text]"), "{tf} → {s}");
            }
        }
    }
}

#[cfg(test)]
mod path_family_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::{
        GlobFilesArgs, ListTreeArgs, MarkdownCheckLinksArgs, ReadDirEnhancedArgs,
    };
    use serde_json::json;

    // 这批工具旧实现是「字段子集结构体 + 反序列化失败即无摘要」，所以要钉住的不是快乐路径，
    // 而是两条边界：缺省 / `null` / 空白回落 `"."`，以及类型不对时仍然**不出**摘要。

    #[test]
    fn copy_and_move_share_args_but_keep_their_own_verb() {
        let v = json!({ "from": " a.txt ", "to": "b/c.txt", "overwrite": true });
        assert_eq!(
            summary_copy_file(&v).as_deref(),
            Some("copy file: a.txt → b/c.txt")
        );
        assert_eq!(
            summary_move_file(&v).as_deref(),
            Some("move file: a.txt → b/c.txt")
        );
    }

    #[test]
    fn copy_and_move_need_both_paths() {
        assert!(summary_copy_file(&json!({ "from": "a.txt" })).is_none());
        assert!(summary_move_file(&json!({ "from": " ", "to": "b.txt" })).is_none());
    }

    #[test]
    fn read_dir_defaults_to_dot_for_missing_null_or_blank_path() {
        for v in [json!({}), json!({ "path": null }), json!({ "path": "   " })] {
            assert_eq!(
                ReadDirEnhancedArgs::summary_line(&v).as_deref(),
                Some("read dir: ."),
                "{v}"
            );
        }
        assert_eq!(
            ReadDirEnhancedArgs::summary_line(&json!({ "path": " src/tools " })).as_deref(),
            Some("read dir: src/tools")
        );
    }

    /// `path` 类型不对：旧 `Option<String>` 反序列化整体失败 ⇒ 没有摘要，迁移不放宽这条。
    #[test]
    fn non_string_path_still_has_no_summary() {
        assert!(ReadDirEnhancedArgs::summary_line(&json!({ "path": 7 })).is_none());
        assert!(ListTreeArgs::summary_line(&json!({ "path": ["src"] })).is_none());
    }

    #[test]
    fn glob_requires_pattern_and_defaults_root_to_dot() {
        assert!(GlobFilesArgs::summary_line(&json!({ "path": "src" })).is_none());
        assert_eq!(
            GlobFilesArgs::summary_line(&json!({ "pattern": " **/*.rs ", "path": "src" })).as_deref(),
            Some("glob **/*.rs @ src")
        );
        assert_eq!(
            GlobFilesArgs::summary_line(&json!({ "pattern": "**/*.rs" })).as_deref(),
            Some("glob **/*.rs @ .")
        );
    }

    #[test]
    fn list_tree_defaults_root_to_dot() {
        assert_eq!(
            ListTreeArgs::summary_line(&json!({})).as_deref(),
            Some("list tree: .")
        );
        assert_eq!(
            ListTreeArgs::summary_line(&json!({ "path": " docs " })).as_deref(),
            Some("list tree: docs")
        );
    }

    #[test]
    fn markdown_roots_fall_back_to_builtin_list_when_absent_or_blank() {
        assert_eq!(
            MarkdownCheckLinksArgs::summary_line(&json!({})).as_deref(),
            Some("markdown link check: README.md, docs")
        );
        assert_eq!(
            MarkdownCheckLinksArgs::summary_line(&json!({ "roots": ["  ", ""] })).as_deref(),
            Some("markdown link check: README.md, docs")
        );
        assert_eq!(
            MarkdownCheckLinksArgs::summary_line(&json!({ "roots": [" docs ", "", "README.md"] }))
                .as_deref(),
            Some("markdown link check: docs, README.md")
        );
    }

    /// `roots` 显式 `null` / 非数组 / 含非字符串：旧 `Vec<String>` 会整体反序列化失败。
    #[test]
    fn markdown_roots_of_wrong_shape_has_no_summary() {
        assert!(MarkdownCheckLinksArgs::summary_line(&json!({ "roots": null })).is_none());
        assert!(MarkdownCheckLinksArgs::summary_line(&json!({ "roots": "docs" })).is_none());
        assert!(MarkdownCheckLinksArgs::summary_line(&json!({ "roots": ["docs", 1] })).is_none());
    }
}
