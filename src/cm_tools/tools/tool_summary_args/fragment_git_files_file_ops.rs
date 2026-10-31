// ── file ops ──────────────────────────────────────────────────

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
