// ── Node.js ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct NpmRunSummaryArgs {
    script: String,
}

impl ToolSummaryLine for NpmRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let script = self.script.trim();
        if script.is_empty() {
            return None;
        }
        Some(format!("npm run {}", script))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct NpxRunSummaryArgs {
    package: String,
}

impl ToolSummaryLine for NpxRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pkg = self.package.trim();
        if pkg.is_empty() {
            return None;
        }
        Some(format!("npx {}", pkg))
    }
}

// ── Process & ports ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct PortCheckSummaryArgs {
    port: u64,
}

impl ToolSummaryLine for PortCheckSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(format!("port check: {}", self.port))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ProcessListSummaryArgs {
    #[serde(default)]
    filter: Option<String>,
}

impl ToolSummaryLine for ProcessListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let filter = self.filter.as_deref().unwrap_or("").trim();
        if filter.is_empty() {
            Some("list processes".to_string())
        } else {
            Some(format!("list processes (filter: {})", filter))
        }
    }
}

// ── Code metrics ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CodeStatsSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for CodeStatsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or(".").trim();
        Some(format!("code stats: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DependencyGraphSummaryArgs {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

impl ToolSummaryLine for DependencyGraphSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let format = self.format.as_deref().unwrap_or("mermaid");
        let kind = self.kind.as_deref().unwrap_or("auto");
        Some(format!("dependency graph ({}/{})", kind, format))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CoverageReportSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for CoverageReportSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some("coverage report (auto-detect)".to_string())
        } else {
            Some(format!("coverage report: {}", path))
        }
    }
}

// ── More file tools ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct DeleteDirSummaryArgs {
    path: String,
    #[serde(default)]
    recursive: bool,
}

impl ToolSummaryLine for DeleteDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        if self.recursive {
            Some(format!("delete directory (recursive): {}", path))
        } else {
            Some(format!("delete directory: {}", path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchReplaceSummaryArgs {
    path: String,
    search: String,
    #[serde(default = "default_true")]
    dry_run: bool,
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
        Some(format!(
            "search-replace{}: {} / \"{}\"",
            if self.dry_run { " (preview)" } else { "" },
            path,
            short
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ChmodFileSummaryArgs {
    path: String,
    mode: String,
}

impl ToolSummaryLine for ChmodFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        let mode = self.mode.trim();
        if path.is_empty() || mode.is_empty() {
            return None;
        }
        Some(format!("chmod: {} → {}", path, mode))
    }
}

