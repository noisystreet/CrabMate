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
