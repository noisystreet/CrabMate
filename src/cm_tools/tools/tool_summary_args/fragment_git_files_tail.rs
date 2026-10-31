// ── Background jobs ───────────────────────────────────────────

// ── Code metrics ──────────────────────────────────────────────
// ── More file tools ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct SearchReplaceSummaryArgs {
    path: String,
    search: String,
    #[serde(default = "default_true")]
    dry_run: bool,
    /// 省略/`0`/`replace_all=true` 的语义差异要在摘要里看得见：默认是「唯一匹配守卫」，
    /// 全量替换属于显式声明，混在一起就等于把「改多了」藏进一行摘要。
    #[serde(default)]
    max_replacements: Option<u64>,
    #[serde(default)]
    replace_all: bool,
}

impl ToolSummaryLine for SearchReplaceSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let search = self.search.trim();
        if search.is_empty() {
            return None;
        }
        let short = if search.chars().count() > 30 {
            format!("{}…", search.chars().take(30).collect::<String>())
        } else {
            search.to_string()
        };
        let scope = match (self.max_replacements, self.replace_all) {
            (None, false) => "",
            (Some(0), _) | (None, true) => " (all)",
            (Some(n), _) => return Some(format!("search_replace (first {n}): {path} / \"{short}\"")),
        };
        Some(format!(
            "search_replace{}{}: {} / \"{}\"",
            if self.dry_run { " (preview)" } else { "" },
            scope,
            path,
            short
        ))
    }
}

