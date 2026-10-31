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

#[derive(Debug, Deserialize)]
pub(super) struct BackgroundJobWaitSummaryArgs {
    #[serde(default)]
    tool_job_id: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    output_matches: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

impl ToolSummaryLine for BackgroundJobWaitSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let BackgroundJobWaitSummaryArgs {
            tool_job_id,
            until,
            output_matches,
            timeout_secs,
        } = self;
        let id = tool_job_id.as_deref().unwrap_or("").trim();
        let until = until.as_deref().unwrap_or("exit").trim();
        let target = match until {
            "output_matches" => match output_matches.as_deref().map(str::trim) {
                Some(p) if !p.is_empty() => format!("output matches `{p}`"),
                _ => "output matches".to_string(),
            },
            "" | "exit" => "exit".to_string(),
            other => other.to_string(),
        };
        let base = if id.is_empty() {
            format!("wait background job until {target}")
        } else {
            format!("wait background job {id} until {target}")
        };
        Some(match timeout_secs {
            Some(secs) => format!("{base} (timeout: {secs}s)"),
            None => base,
        })
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
