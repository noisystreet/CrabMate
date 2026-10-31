// ── codebase_semantic_search ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CodebaseSemanticSearchSummaryArgs {
    #[serde(default)]
    rebuild_index: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    incremental: Option<bool>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    retrieve_mode: Option<String>,
}

impl ToolSummaryLine for CodebaseSemanticSearchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.rebuild_index {
            let p = self.path.as_deref().unwrap_or(".").trim();
            let p = if p.is_empty() { "." } else { p };
            return Some(match self.incremental {
                Some(false) => format!("semantic index rebuild full ({})", p),
                _ => format!("semantic index rebuild ({})", p),
            });
        }
        let q = self.query.as_deref().unwrap_or("").trim();
        if q.is_empty() {
            return Some("semantic code search".to_string());
        }
        let mut s: String = q.chars().take(48).collect();
        if q.chars().count() > 48 {
            s.push('…');
        }
        let mode = self
            .retrieve_mode
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("hybrid");
        Some(format!("code search [{}]: {}", mode, s))
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

/// `rust_analyzer` 的摘要行：同一结构体接住所有 action（原 8 个近乎相同的
/// `RustAnalyzer*SummaryArgs` 已合并；差异只在文案前缀与是否需要 `line`）。
#[derive(Debug, Deserialize)]
pub(super) struct RustAnalyzerSummaryArgs {
    #[serde(default)]
    action: String,
    path: String,
    #[serde(default)]
    line: Option<u64>,
    #[serde(default)]
    query: Option<String>,
}

impl ToolSummaryLine for RustAnalyzerSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        match self.action.trim() {
            "workspace_symbol" => {
                let q = self.query.as_deref().unwrap_or("").trim();
                (!q.is_empty()).then(|| format!("rust-analyzer workspace/symbol {q} (prime {path})"))
            }
            "document_symbol" => Some(format!("rust-analyzer document symbols {path}")),
            action => {
                let label = match action {
                    "goto_definition" => "goto definition",
                    "find_references" => "find references",
                    "hover" => "hover",
                    "goto_implementation" => "goto implementation",
                    "goto_type_definition" => "goto type definition",
                    "document_highlight" => "document highlight",
                    // 缺 action 时仍给一行可读文案（不静默丢卡片）。
                    "" => "request",
                    other => other,
                };
                Some(format!(
                    "rust-analyzer {label} {}:{}",
                    path,
                    self.line.unwrap_or(0)
                ))
            }
        }
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
#[cfg(test)]
mod rust_analyzer_summary_tests {
    use super::*;

    fn line(v: serde_json::Value) -> Option<String> {
        serde_json::from_value::<RustAnalyzerSummaryArgs>(v)
            .ok()?
            .summary_line()
    }

    /// 合并后一个结构体要覆盖全部 action 的文案（原为 8 个独立 summary 结构）。
    #[test]
    fn rust_analyzer_summary_covers_actions() {
        assert_eq!(
            line(serde_json::json!({"action":"goto_definition","path":"a.rs","line":3})).as_deref(),
            Some("rust-analyzer goto definition a.rs:3")
        );
        assert_eq!(
            line(serde_json::json!({"action":"find_references","path":"a.rs","line":7})).as_deref(),
            Some("rust-analyzer find references a.rs:7")
        );
        assert_eq!(
            line(serde_json::json!({"action":"document_symbol","path":"a.rs"})).as_deref(),
            Some("rust-analyzer document symbols a.rs")
        );
        assert_eq!(
            line(serde_json::json!({"action":"workspace_symbol","path":"a.rs","query":"Foo"}))
                .as_deref(),
            Some("rust-analyzer workspace/symbol Foo (prime a.rs)")
        );
        // `line` 缺失时仍出摘要（与逐工具时代一致：unwrap_or(0)）。
        assert_eq!(
            line(serde_json::json!({"action":"hover","path":"a.rs"})).as_deref(),
            Some("rust-analyzer hover a.rs:0")
        );
    }

    #[test]
    fn rust_analyzer_summary_omits_when_uninformative() {
        // 缺 query 的 workspace_symbol、空 path 都不生成摘要行。
        assert_eq!(
            line(serde_json::json!({"action":"workspace_symbol","path":"a.rs"})),
            None
        );
        assert_eq!(line(serde_json::json!({"action":"hover","path":"  "})), None);
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
