// ── Background jobs ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct BackgroundJobSummaryArgs {
    #[serde(default)]
    action: String,
    #[serde(default)]
    tool_job_id: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

impl ToolSummaryLine for BackgroundJobSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let BackgroundJobSummaryArgs {
            action,
            tool_job_id,
            limit,
        } = self;
        let id = tool_job_id.as_deref().unwrap_or("").trim();
        match action.trim() {
            "status" => Some(if id.is_empty() {
                "background job status".to_string()
            } else {
                format!("background job status: {id}")
            }),
            "list" => Some(match limit {
                Some(n) => format!("list background jobs (limit: {n})"),
                None => "list background jobs".to_string(),
            }),
            "output" => Some(if id.is_empty() {
                "background job output".to_string()
            } else {
                format!("background job output: {id}")
            }),
            "" => Some("background job".to_string()),
            other => Some(format!("background job: {other}")),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct BackgroundJobCancelSummaryArgs {
    tool_job_id: String,
}

impl ToolSummaryLine for BackgroundJobCancelSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let id = self.tool_job_id.trim();
        if id.is_empty() {
            Some("cancel background job".to_string())
        } else {
            Some(format!("cancel background job: {id}"))
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
            "search_replace{}: {} / \"{}\"",
            if self.dry_run { " (preview)" } else { "" },
            path,
            short
        ))
    }
}

#[cfg(test)]
mod background_job_summary_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_summary_uses_job_id() {
        let v = json!({ "action": "status", "tool_job_id": " j1 " });
        let s = summarize_from_value::<BackgroundJobSummaryArgs>(&v).expect("summary");
        assert_eq!(s, "background job status: j1");
    }

    #[test]
    fn list_summary_reports_limit() {
        let v = json!({ "action": "list", "limit": 5 });
        let s = summarize_from_value::<BackgroundJobSummaryArgs>(&v).expect("summary");
        assert_eq!(s, "list background jobs (limit: 5)");
    }

    #[test]
    fn missing_action_falls_back_to_generic_line() {
        let v = json!({ "tool_job_id": "j1" });
        let s = summarize_from_value::<BackgroundJobSummaryArgs>(&v).expect("summary");
        assert_eq!(s, "background job");
    }
}
