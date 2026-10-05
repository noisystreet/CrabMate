// ── git diff * ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    stat: Option<bool>,
    #[serde(default)]
    name_only: Option<bool>,
    #[serde(default)]
    base: Option<String>,
}

impl ToolSummaryLine for GitDiffSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let flags = if self.stat.unwrap_or(false) {
            " --stat"
        } else if self.name_only.unwrap_or(false) {
            " --name-only"
        } else {
            ""
        };
        let scope = match self
            .base
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(base) => format!("{}...HEAD", base),
            None => self.mode.as_deref().unwrap_or("working").to_string(),
        };
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff{} ({})", flags, scope))
        } else {
            Some(format!("git diff{} ({}): {}", flags, scope, path))
        }
    }
}
