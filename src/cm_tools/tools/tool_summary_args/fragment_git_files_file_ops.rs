// ── file ops ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CreateFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for CreateFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("create file: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ModifyFileSummaryArgs {
    path: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
}

impl ToolSummaryLine for ModifyFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let mode = self.mode.as_deref().unwrap_or("full");
        if mode == "replace_lines" {
            Some(format!(
                "replace lines {}-{} in {}",
                self.start_line.unwrap_or(0),
                self.end_line.unwrap_or(0),
                path
            ))
        } else {
            Some(format!("modify file: {}", path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CopyFileSummaryArgs {
    from: String,
    to: String,
}

impl ToolSummaryLine for CopyFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let from = self.from.trim();
        let to = self.to.trim();
        if from.is_empty() || to.is_empty() {
            return None;
        }
        Some(format!("copy file: {} → {}", from, to))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct MoveFileSummaryArgs {
    from: String,
    to: String,
}

impl ToolSummaryLine for MoveFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let from = self.from.trim();
        let to = self.to.trim();
        if from.is_empty() || to.is_empty() {
            return None;
        }
        Some(format!("move file: {} → {}", from, to))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ReadDirSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for ReadDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or(".").trim();
        let p = if path.is_empty() { "." } else { path };
        Some(format!("read dir: {}", p))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct WebSearchSummaryArgs {
    query: String,
}

impl ToolSummaryLine for WebSearchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let q = self.query.trim();
        if q.is_empty() {
            return None;
        }
        Some(format!("web search: {}", q))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HttpFetchSummaryArgs {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    text_format: Option<String>,
}

impl ToolSummaryLine for HttpFetchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let u = self.url.trim();
        if u.is_empty() {
            return None;
        }
        let m = self
            .method
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("GET");
        let tf = self
            .text_format
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase().replace('-', "_"));
        let suffix = match tf.as_deref() {
            Some("html_text" | "htmltext" | "text") => " [html_text]",
            _ => "",
        };
        Some(format!("HTTP {} {}{}", m.to_ascii_uppercase(), u, suffix))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HttpRequestSummaryArgs {
    url: String,
    method: String,
    #[serde(default)]
    text_format: Option<String>,
}

impl ToolSummaryLine for HttpRequestSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let u = self.url.trim();
        let m = self.method.trim();
        if u.is_empty() || m.is_empty() {
            return None;
        }
        let tf = self
            .text_format
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase().replace('-', "_"));
        let suffix = match tf.as_deref() {
            Some("html_text" | "htmltext" | "text") => " [html_text]",
            _ => "",
        };
        Some(format!("HTTP {} {}{}", m.to_ascii_uppercase(), u, suffix))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GlobFilesSummaryArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GlobFilesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pat = self.pattern.trim();
        if pat.is_empty() {
            return None;
        }
        let root = self.path.as_deref().unwrap_or(".").trim();
        Some(format!(
            "glob {} @ {}",
            pat,
            if root.is_empty() { "." } else { root }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct MarkdownCheckLinksSummaryArgs {
    #[serde(default)]
    roots: Vec<String>,
}

impl ToolSummaryLine for MarkdownCheckLinksSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let roots = self
            .roots
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let roots = if roots.is_empty() {
            "README.md, docs".to_string()
        } else {
            roots
        };
        Some(format!("markdown link check: {}", roots))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredSummaryArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    path_a: Option<String>,
    #[serde(default)]
    path_b: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    mutation: Option<String>,
}

impl ToolSummaryLine for StructuredSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = trimmed_opt(self.path.as_deref());
        match self.action.as_deref().unwrap_or("").trim() {
            "validate" => Some(format!("structured validate: {}", path?)),
            "query" => {
                let q = trimmed_opt(self.query.as_deref())?;
                Some(format!("structured query: {} [{}]", path?, q))
            }
            "diff" => {
                let a = trimmed_opt(self.path_a.as_deref())?;
                let b = trimmed_opt(self.path_b.as_deref())?;
                Some(format!("structured diff: {} vs {}", a, b))
            }
            "patch" => {
                let q = trimmed_opt(self.query.as_deref())?;
                let m = self.mutation.as_deref().unwrap_or("set").trim();
                Some(format!("structured patch: {} [{} @ {}]", path?, m, q))
            }
            _ => None,
        }
    }
}

fn trimmed_opt(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

#[derive(Debug, Deserialize)]
pub(super) struct ListTreeSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for ListTreeSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let root = self.path.as_deref().unwrap_or(".").trim();
        Some(format!(
            "list tree: {}",
            if root.is_empty() { "." } else { root }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ApplyPatchSummaryArgs {
    patch: String,
}

impl ToolSummaryLine for ApplyPatchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let files = self
            .patch
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
}

/// `code_nav` 的摘要行：一个结构体接住四个 action（原四个 `*SummaryArgs` 已合并；
/// 差异只在文案前缀与读哪个字段）。缺/未知 action 时给中性文案，不静默丢卡片。
#[derive(Debug, Deserialize)]
pub(super) struct CodeNavSummaryArgs {
    #[serde(default)]
    action: String,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for CodeNavSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.trim().to_string();
        // 只做分发：每个 action 的文案各自一个小函数，避免这里堆成分支丛林（lizard CCN≤10）。
        match action.as_str() {
            "file_outline" => outline_line(self.path.as_deref()),
            "find_references" => references_line(self.symbol.as_deref()),
            "call_graph_sketch" => sketch_line(self.symbol.as_deref(), self.symbols),
            "find_symbol" => symbol_line(self.symbol.as_deref()),
            other => unknown_action_line(other, self.symbol.as_deref()),
        }
    }
}

fn outline_line(path: Option<&str>) -> Option<String> {
    let path = path.unwrap_or("").trim();
    (!path.is_empty()).then(|| format!("Rust outline: {path}"))
}

fn references_line(symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    (!s.is_empty()).then(|| format!("find references: {s}"))
}

fn symbol_line(symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    (!s.is_empty()).then(|| format!("find symbol: {s}"))
}

/// 缺 / 未知 action：中性文案，不把一次注定失败的调用显示成成功的 `find_symbol`。
fn unknown_action_line(action: &str, symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    if s.is_empty() {
        return None;
    }
    Some(if action.is_empty() {
        format!("code_nav: {s}")
    } else {
        format!("code_nav {action}: {s}")
    })
}

fn sketch_line(symbol: Option<&str>, symbols: Vec<String>) -> Option<String> {
    let mut v: Vec<String> = symbols
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(s) = symbol.map(str::trim)
        && !s.is_empty()
    {
        v.push(s.to_string());
    }
    v.sort();
    v.dedup();
    if v.is_empty() {
        return None;
    }
    const MAX: usize = 3;
    let head: Vec<_> = v.iter().take(MAX).cloned().collect();
    let mut out = format!("impact sketch: {}", head.join(", "));
    if v.len() > MAX {
        out.push_str(&format!(" +{}", v.len() - MAX));
    }
    Some(out)
}

#[cfg(test)]
mod code_nav_summary_tests {
    use super::*;

    fn line(v: serde_json::Value) -> Option<String> {
        serde_json::from_value::<CodeNavSummaryArgs>(v)
            .ok()?
            .summary_line()
    }

    #[test]
    fn covers_all_four_actions() {
        assert_eq!(
            line(serde_json::json!({"action":"find_symbol","symbol":"run_tool"})).as_deref(),
            Some("find symbol: run_tool")
        );
        assert_eq!(
            line(serde_json::json!({"action":"find_references","symbol":"run_tool"})).as_deref(),
            Some("find references: run_tool")
        );
        assert_eq!(
            line(serde_json::json!({"action":"file_outline","path":"src/a.rs"})).as_deref(),
            Some("Rust outline: src/a.rs")
        );
        assert_eq!(
            line(serde_json::json!({
                "action": "call_graph_sketch",
                "symbols": ["b", "a"],
                "symbol": "c"
            }))
            .as_deref(),
            Some("impact sketch: a, b, c")
        );
    }

    #[test]
    fn blank_inputs_yield_no_line() {
        assert_eq!(line(serde_json::json!({"action":"find_symbol"})), None);
        assert_eq!(line(serde_json::json!({"action":"file_outline"})), None);
        assert_eq!(line(serde_json::json!({"action":"call_graph_sketch"})), None);
    }

    #[test]
    fn unknown_action_is_not_labelled_as_find_symbol() {
        // 缺 / 未知 action 曾复用 `find symbol:` 文案，与真实失败原因不符。
        assert_eq!(
            line(serde_json::json!({"action":"nope","symbol":"x"})).as_deref(),
            Some("code_nav nope: x")
        );
        assert_eq!(
            line(serde_json::json!({"symbol":"x"})).as_deref(),
            Some("code_nav: x")
        );
    }
}
