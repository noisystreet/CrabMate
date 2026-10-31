// ── gh * ──────────────────────────────────────────────────────

fn gh_repo_suffix(repo: Option<String>) -> String {
    repo.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({})", r))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrSummaryArgs {
    action: String,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    merge_method: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    draft: Option<bool>,
}

impl ToolSummaryLine for GhPrSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.trim();
        if action.is_empty() {
            return None;
        }
        let mut line = format!("gh pr {action}");
        if let Some(n) = self.number.filter(|n| *n > 0) {
            line.push_str(&format!(" #{n}"));
        }
        match action {
            "merge" => {
                let method = self.merge_method.as_deref().unwrap_or("rebase");
                line.push_str(&format!(" ({method})"));
            }
            "review" => {
                let ev = self.event.as_deref().unwrap_or("").trim().to_string();
                if !ev.is_empty() {
                    line.push_str(&format!(" {ev}"));
                }
            }
            "create" if self.draft == Some(true) => line.push_str(" draft"),
            _ => {}
        }
        line.push_str(&gh_repo_suffix(self.repo));
        // 只有 create / edit 带标题，其余动作的 title 字段为空。
        let title = self.title.unwrap_or_default();
        let t = title.trim();
        if !t.is_empty() {
            let mut head: String = t.chars().take(40).collect();
            if t.chars().count() > 40 {
                head.push('\u{2026}');
            }
            line.push_str(&format!(": {head}"));
        }
        Some(line)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhRunSummaryArgs {
    action: String,
    run_id: String,
    #[serde(default)]
    failed: Option<bool>,
}

impl ToolSummaryLine for GhRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let id = self.run_id.trim();
        if id.is_empty() {
            return None;
        }
        let action = self.action.trim();
        match action {
            "rerun" => Some(format!(
                "gh run rerun {}{}",
                id,
                if self.failed == Some(true) {
                    " --failed"
                } else {
                    ""
                }
            )),
            "failure_summary" => Some(format!("gh run failure summary {id}")),
            _ => Some(format!("gh run {action} {id}")),
        }
    }
}
