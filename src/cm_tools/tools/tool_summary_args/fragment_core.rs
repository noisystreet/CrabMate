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

#[derive(Debug, Deserialize)]
pub(super) struct SearchInFilesSummaryArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for SearchInFilesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return None;
        }
        const MAX_PATTERN_CHARS: usize = 40;
        let mut pat: String = pattern.chars().take(MAX_PATTERN_CHARS).collect();
        if pattern.chars().count() > MAX_PATTERN_CHARS {
            pat.push('…');
        }
        const MAX_PATH_CHARS: usize = 28;
        let sub = self
            .path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        Some(match sub {
            Some(p) => {
                let mut ps: String = p.chars().take(MAX_PATH_CHARS).collect();
                if p.chars().count() > MAX_PATH_CHARS {
                    ps.push('…');
                }
                format!("search in files: {} @ {}", pat, ps)
            }
            None => format!("search in files: {}", pat),
        })
    }
}

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

#[derive(Debug, Deserialize)]
pub(super) struct RunCommandSummaryArgs {
    /// 模型常把整条命令写成数组（`["git","status"]`）；字段声明为 `String` 时反序列化会
    /// 整体失败，摘要退化成占位串 `tool: <name>`，客户端只能再解析一遍参数才补回来。
    command: serde_json::Value,
    /// 模型常把 `args` 写成数组或整段字符串；缺省为 JSON null。
    #[serde(default)]
    args: serde_json::Value,
}

impl ToolSummaryLine for RunCommandSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mut tokens = command_display_tokens(&self.command);
        if tokens.is_empty() {
            return None;
        }
        tokens.extend(run_command_arg_display_tokens(&self.args));
        Some(tokens.join(" "))
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

#[derive(Debug, Deserialize)]
pub(super) struct TerminalSessionSummaryArgs {
    action: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
    /// 与工具 JSON 一致；摘要里拼接为类 shell 命令行，供 Web 终端卡展示。
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

impl ToolSummaryLine for TerminalSessionSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let a = self.action.trim().to_ascii_lowercase();
        match a.as_str() {
            "list" => Some("terminal_session list".to_string()),
            "close" => Some(format!(
                "terminal_session close {}",
                self.session_id.as_deref().unwrap_or("?")
            )),
            "resize" => Some(format!(
                "terminal_session resize {}",
                self.session_id.as_deref().unwrap_or("?")
            )),
            "send_signal" => Some(format!(
                "terminal_session signal {}",
                self.session_id.as_deref().unwrap_or("?")
            )),
            "interrupt" => Some(format!(
                "terminal_session interrupt {}",
                self.session_id.as_deref().unwrap_or("?")
            )),
            "exec" => {
                let sid = self.session_id.as_deref().unwrap_or("").trim();
                let cmd = self.command.as_deref().unwrap_or("").trim();
                if !sid.is_empty() {
                    Some(format!("terminal_session exec sid={sid}"))
                } else if !cmd.is_empty() {
                    let tail = self
                        .args
                        .iter()
                        .filter_map(|x| x.as_str())
                        .map(terminal_session_arg_display_token)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if tail.is_empty() {
                        Some(format!("terminal_session exec {cmd}"))
                    } else {
                        Some(format!("terminal_session exec {cmd} {tail}"))
                    }
                } else {
                    Some("terminal_session exec".to_string())
                }
            }
            _ => Some(format!("terminal_session {}", self.action)),
        }
    }
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

#[derive(Debug, Deserialize)]
pub(super) struct PythonSnippetRunSummaryArgs {
    #[serde(default)]
    use_uv: bool,
}

impl ToolSummaryLine for PythonSnippetRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(if self.use_uv {
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
