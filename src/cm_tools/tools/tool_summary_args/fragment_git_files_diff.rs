// ── git diff * ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff ({})", mode))
        } else {
            Some(format!("git diff ({}): {}", mode, path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffStatSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffStatSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff --stat ({})", mode))
        } else {
            Some(format!("git diff --stat ({}): {}", mode, path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffNamesSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffNamesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff --name-only ({})", mode))
        } else {
            Some(format!("git diff --name-only ({}): {}", mode, path))
        }
    }
}
