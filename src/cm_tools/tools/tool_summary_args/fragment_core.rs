// ── codebase_semantic_search ──────────────────────────────────

/// `codebase_semantic_search`：旧结构体五个字段全带 `#[serde(default)]`（缺键宽容、类型不符
/// 整体失败）。先把五个字段取完再分支，别因为当前走 query 分支就跳过 `incremental` 的校验。
impl ToolArgsSummary for super::tool_param_types::CodebaseSemanticSearchArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let rebuild_index = arg_bool_default(args, "rebuild_index")?;
        let path = arg_opt_str(args, "path")?;
        let incremental = arg_opt_bool(args, "incremental")?;
        let query = arg_opt_str(args, "query")?;
        let retrieve_mode = arg_opt_str(args, "retrieve_mode")?;
        if rebuild_index {
            let p = path.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(".");
            return Some(match incremental {
                Some(false) => format!("semantic index rebuild full ({p})"),
                _ => format!("semantic index rebuild ({p})"),
            });
        }
        let q = query.unwrap_or("").trim();
        if q.is_empty() {
            return Some("semantic code search".to_string());
        }
        let mut s: String = q.chars().take(48).collect();
        if q.chars().count() > 48 {
            s.push('…');
        }
        let mode = retrieve_mode
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("hybrid");
        Some(format!("code search [{mode}]: {s}"))
    }
}

// ── search_in_files ─────────────────────────────────────────

// ── run_command (`args` may be array or string) ─

fn json_arg_display_token(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(terminal_session_arg_display_token(s)),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        other => Some(terminal_session_arg_display_token(&other.to_string())),
    }
}

fn run_command_arg_display_tokens(args: &serde_json::Value) -> Vec<String> {
    match args {
        serde_json::Value::Null => Vec::new(),
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![terminal_session_arg_display_token(t)]
            }
        }
        serde_json::Value::Array(items) => items.iter().filter_map(json_arg_display_token).collect(),
        other => vec![terminal_session_arg_display_token(&other.to_string())],
    }
}

/// `command` 的展示 token：数组即一整条命令行，字符串原样保留（它本身就是要显示的命令，
/// 加引号反而奇怪）。参数位置另由 [`run_command_arg_display_tokens`] 逐 token 处理。
fn command_display_tokens(command: &serde_json::Value) -> Vec<String> {
    match command {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![t.to_string()]
            }
        }
        serde_json::Value::Array(items) => items.iter().filter_map(json_arg_display_token).collect(),
        serde_json::Value::Null => Vec::new(),
        other => vec![terminal_session_arg_display_token(&other.to_string())],
    }
}

fn terminal_session_arg_display_token(arg: &str) -> String {
    let arg = arg.trim();
    if arg.is_empty() {
        return "''".to_string();
    }
    let safe_unquoted = arg.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@')
    }) && !arg.contains('\'');
    if safe_unquoted {
        return arg.to_string();
    }
    let mut s = String::with_capacity(arg.len().saturating_add(2));
    s.push('"');
    for ch in arg.chars() {
        if ch == '"' || ch == '\\' {
            s.push('\\');
        }
        s.push(ch);
    }
    s.push('"');
    s
}

// ── rust_analyzer ──────────────────────────────────────

/// `rust_analyzer` 的摘要行：一个 `impl` 接住所有 action（原 8 个近乎相同的
/// `RustAnalyzer*SummaryArgs` 已合并；差异只在文案前缀与是否需要 `line`）。
///
/// 旧结构体里 `action` 有 `#[serde(default)]`、`path` 是**必填**、`line` / `query` 是
/// `Option`，但四个字段都会先反序列化——与当前 action 无关的字段类型不符同样没有摘要。
impl ToolArgsSummary for super::tool_param_types::RustAnalyzerArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action = arg_str_default(args, "action")?;
        let path = arg_str_raw(args, "path")?;
        let line = arg_opt_u64(args, "line")?;
        let query = arg_opt_str(args, "query")?;
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        rust_analyzer_line(action.trim(), path, line, query)
    }
}

fn rust_analyzer_line(
    action: &str,
    path: &str,
    line: Option<u64>,
    query: Option<&str>,
) -> Option<String> {
    match action {
        "workspace_symbol" => {
            let q = query.unwrap_or("").trim();
            (!q.is_empty()).then(|| format!("rust-analyzer workspace/symbol {q} (prime {path})"))
        }
        "document_symbol" => Some(format!("rust-analyzer document symbols {path}")),
        other => Some(format!(
            "rust-analyzer {} {path}:{}",
            rust_analyzer_label(other),
            line.unwrap_or(0)
        )),
    }
}

fn rust_analyzer_label(action: &str) -> &str {
    match action {
        "goto_definition" => "goto definition",
        "find_references" => "find references",
        "hover" => "hover",
        "goto_implementation" => "goto implementation",
        "goto_type_definition" => "goto type definition",
        "document_highlight" => "document highlight",
        // 缺 action 时仍给一行可读文案（不静默丢卡片）。
        "" => "request",
        other => other,
    }
}

// ── python ───────────────────────────────────────────────────

/// 旧的 `PythonSnippetRunSummaryArgs` 只有 `use_uv: bool` + `#[serde(default)]`：
/// 缺键 ⇒ false；`null` / 类型不符 ⇒ 整条摘要消失。
impl ToolArgsSummary for super::tool_param_types::PythonSnippetRunArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let use_uv = arg_bool_default(args, "use_uv")?;
        Some(if use_uv {
            "python snippet (uv)".to_string()
        } else {
            "python snippet".to_string()
        })
    }
}

fn default_true() -> bool {
    true
}


// 各 fragment 被 `include!` 进同一模块，`tests` 名字不能复用，故带前缀。
// 各 fragment 被 `include!` 进同一模块，`tests` 名字不能复用，故带前缀。
#[cfg(test)]
mod rust_analyzer_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::RustAnalyzerArgs;
    use serde_json::json;

    fn line(v: serde_json::Value) -> Option<String> {
        RustAnalyzerArgs::summary_line(&v)
    }

    /// 合并后一个 `impl` 要覆盖全部 action 的文案（原为 8 个独立 summary 结构）。
    #[test]
    fn rust_analyzer_summary_covers_actions() {
        assert_eq!(
            line(json!({"action":"goto_definition","path":"a.rs","line":3})).as_deref(),
            Some("rust-analyzer goto definition a.rs:3")
        );
        assert_eq!(
            line(json!({"action":"find_references","path":"a.rs","line":7})).as_deref(),
            Some("rust-analyzer find references a.rs:7")
        );
        assert_eq!(
            line(json!({"action":"document_symbol","path":"a.rs"})).as_deref(),
            Some("rust-analyzer document symbols a.rs")
        );
        assert_eq!(
            line(json!({"action":"workspace_symbol","path":"a.rs","query":"Foo"})).as_deref(),
            Some("rust-analyzer workspace/symbol Foo (prime a.rs)")
        );
        // `line` 缺失时仍出摘要（与逐工具时代一致：unwrap_or(0)）。
        assert_eq!(
            line(json!({"action":"hover","path":"a.rs"})).as_deref(),
            Some("rust-analyzer hover a.rs:0")
        );
        // 缺 action ⇒ 中性文案，不丢卡片。
        assert_eq!(
            line(json!({"path":"a.rs"})).as_deref(),
            Some("rust-analyzer request a.rs:0")
        );
    }

    #[test]
    fn rust_analyzer_summary_omits_when_uninformative() {
        // 缺 query 的 workspace_symbol、空 path、缺 path 都不生成摘要行。
        assert_eq!(line(json!({"action":"workspace_symbol","path":"a.rs"})), None);
        assert_eq!(line(json!({"action":"hover","path":"  "})), None);
        assert_eq!(line(json!({"action":"hover"})), None);
    }

    /// 与当前 action 无关的字段类型不符同样不给摘要：旧结构体是整体反序列化。
    #[test]
    fn wrong_typed_field_drops_the_line() {
        assert!(line(json!({"action":"hover","path":"a.rs","line":"3"})).is_none());
        assert!(line(json!({"action":"hover","path":"a.rs","query":1})).is_none());
        assert!(line(json!({"action":7,"path":"a.rs"})).is_none());
    }
}

#[cfg(test)]
mod codebase_semantic_search_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::CodebaseSemanticSearchArgs;
    use serde_json::json;

    fn line(v: serde_json::Value) -> Option<String> {
        CodebaseSemanticSearchArgs::summary_line(&v)
    }

    #[test]
    fn rebuild_wins_over_query() {
        assert_eq!(
            line(json!({"rebuild_index": true})).as_deref(),
            Some("semantic index rebuild (.)")
        );
        assert_eq!(
            line(json!({"rebuild_index": true, "path": "  src  "})).as_deref(),
            Some("semantic index rebuild (src)")
        );
        // `incremental: false` 才写 full；`true` / 缺键都走增量文案。
        assert_eq!(
            line(json!({"rebuild_index": true, "incremental": false})).as_deref(),
            Some("semantic index rebuild full (.)")
        );
    }

    #[test]
    fn query_line_truncates_and_shows_mode() {
        assert_eq!(
            line(json!({"query": "  how does auth work  "})).as_deref(),
            Some("code search [hybrid]: how does auth work")
        );
        assert_eq!(
            line(json!({"query": "q", "retrieve_mode": " fts "})).as_deref(),
            Some("code search [fts]: q")
        );
        // 48 字符上限，超出补省略号。
        let long = "a".repeat(50);
        let got = line(json!({"query": long})).unwrap();
        assert_eq!(got, format!("code search [hybrid]: {}…", "a".repeat(48)));
    }

    #[test]
    fn empty_query_falls_back_to_generic_line() {
        assert_eq!(line(json!({})).as_deref(), Some("semantic code search"));
        assert_eq!(line(json!({"query": "   "})).as_deref(), Some("semantic code search"));
    }

    /// 与当前分支无关的字段类型不符同样不给摘要。
    #[test]
    fn wrong_typed_field_drops_the_line() {
        assert!(line(json!({"query": "x", "incremental": "yes"})).is_none());
        assert!(line(json!({"rebuild_index": "true"})).is_none());
        assert!(line(json!({"query": "x", "retrieve_mode": 1})).is_none());
    }
}

#[cfg(test)]
mod python_snippet_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::PythonSnippetRunArgs;
    use serde_json::json;

    #[test]
    fn use_uv_switches_the_label() {
        assert_eq!(
            PythonSnippetRunArgs::summary_line(&json!({"code": "print(1)"})).as_deref(),
            Some("python snippet")
        );
        assert_eq!(
            PythonSnippetRunArgs::summary_line(&json!({"code": "print(1)", "use_uv": true}))
                .as_deref(),
            Some("python snippet (uv)")
        );
        assert_eq!(
            PythonSnippetRunArgs::summary_line(&json!({"code": "print(1)", "use_uv": false}))
                .as_deref(),
            Some("python snippet")
        );
    }

    /// `use_uv` 是 `#[serde(default)] bool`：缺键宽容，`null` / 类型不符整体失败。
    #[test]
    fn wrong_typed_use_uv_drops_the_line() {
        assert!(
            PythonSnippetRunArgs::summary_line(&json!({"code": "x", "use_uv": null})).is_none()
        );
        assert!(
            PythonSnippetRunArgs::summary_line(&json!({"code": "x", "use_uv": "yes"})).is_none()
        );
    }
}
