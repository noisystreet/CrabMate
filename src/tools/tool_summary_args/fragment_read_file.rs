#[derive(Debug, Deserialize)]
pub(super) struct ReadFileSummaryArgs {
    path: String,
    #[serde(default)]
    anchor_line: Option<u64>,
    #[serde(default)]
    context_lines: Option<u64>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
    #[serde(default)]
    max_lines: Option<u64>,
    #[serde(default)]
    encoding: Option<String>,
}

impl ToolSummaryLine for ReadFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let enc = self
            .encoding
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let suffix = match self.anchor_line {
            Some(a) => format!(" [@L{} ±{}]", a, self.context_lines.unwrap_or(120)),
            None => match (self.start_line, self.end_line, self.max_lines) {
                (Some(s), Some(e), _) => {
                    let (lo, hi) = if e < s { (e, s) } else { (s, e) };
                    format!(" [{}-{}]", lo, hi)
                }
                (Some(s), None, Some(m)) => format!(" [{}~ max_lines={}]", s, m),
                (Some(s), None, None) => format!(" [{}~]", s),
                (None, Some(e), _) => format!(" [1-{}]", e),
                (None, None, Some(m)) => format!(" [chunk max_lines={}]", m),
                (None, None, None) => String::new(),
            },
        };
        let enc_s = enc.map(|e| format!(" enc={}", e)).unwrap_or_default();
        Some(format!("read file: {}{}{}", path, suffix, enc_s))
    }
}
