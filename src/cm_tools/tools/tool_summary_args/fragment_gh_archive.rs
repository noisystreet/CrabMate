// ── gh * ──────────────────────────────────────────────────────

fn gh_repo_suffix(repo: Option<String>) -> String {
    repo.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({})", r))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(super) struct GhIssueCreateSummaryArgs {
    title: String,
    #[serde(default)]
    repo: Option<String>,
}

impl ToolSummaryLine for GhIssueCreateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let t = self.title.trim();
        if t.is_empty() {
            return None;
        }
        let mut head: String = t.chars().take(40).collect();
        if t.chars().count() > 40 {
            head.push('…');
        }
        Some(format!(
            "gh issue create{}: {}",
            gh_repo_suffix(self.repo),
            head
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhReleaseCreateSummaryArgs {
    tag: String,
    #[serde(default)]
    draft: Option<bool>,
}

impl ToolSummaryLine for GhReleaseCreateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let tag = self.tag.trim();
        if tag.is_empty() {
            return None;
        }
        let draft = self.draft == Some(true);
        Some(format!(
            "gh release create {}{}",
            tag,
            if draft { " (draft)" } else { "" }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhApiSummaryArgs {
    path: String,
    #[serde(default)]
    method: Option<String>,
}

impl ToolSummaryLine for GhApiSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let method = self
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let mut p: String = path.chars().take(40).collect();
        if path.chars().count() > 40 {
            p.push('…');
        }
        Some(format!("gh api {} {}", method, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_command_summary_joins_command_and_string_args() {
        let v = json!({ "command": "cargo", "args": ["test", "--all"] });
        let s = summarize_from_value::<RunCommandSummaryArgs>(&v).expect("summary");
        assert_eq!(s, "cargo test --all");
    }

    #[test]
    fn run_command_summary_accepts_args_as_single_string() {
        let v = json!({ "command": "bash", "args": "-c echo hi" });
        let s = summarize_from_value::<RunCommandSummaryArgs>(&v).expect("summary");
        assert!(s.starts_with("bash "), "{s}");
        assert!(s.contains("echo hi"), "{s}");
    }

    #[test]
    fn run_command_summary_quotes_args_with_spaces() {
        let v = json!({ "command": "bash", "args": ["-c", "echo hello world"] });
        let s = summarize_from_value::<RunCommandSummaryArgs>(&v).expect("summary");
        assert_eq!(s, r#"bash -c "echo hello world""#);
    }

}

#[derive(Debug, Deserialize)]
pub(super) struct ReadBinaryMetaSummaryArgs {
    path: String,
}

impl ToolSummaryLine for ReadBinaryMetaSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("binary metadata: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteFilesSummaryArgs {
    paths: Vec<String>,
}

impl ToolSummaryLine for DeleteFilesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let n = self.paths.len();
        if n == 0 {
            return None;
        }
        let shown: Vec<String> = self.paths.into_iter().take(3).collect();
        let mut line = format!("delete files: {}", shown.join(", "));
        if n > 3 {
            line.push_str(&format!(" (+{} more)", n - 3));
        }
        Some(line)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct AppendFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for AppendFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("append to file: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateDirSummaryArgs {
    path: String,
}

impl ToolSummaryLine for CreateDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("create directory: {}", path))
    }
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

// ── archive_pack ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ArchivePackSummaryArgs {
    output: String,
    #[serde(default)]
    sources: Vec<String>,
}

impl ToolSummaryLine for ArchivePackSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let output = self.output.trim();
        let count = self.sources.len();
        if output.is_empty() {
            return None;
        }
        Some(format!("pack {} items into {}", count, output))
    }
}

// ── archive_unpack ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ArchiveUnpackSummaryArgs {
    archive: String,
    #[serde(default)]
    output_dir: Option<String>,
}

impl ToolSummaryLine for ArchiveUnpackSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let archive = self.archive.trim();
        if archive.is_empty() {
            return None;
        }
        let dir = self
            .output_dir
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(".");
        Some(format!("unpack {} to {}", archive, dir))
    }
}

// ── archive_list ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ArchiveListSummaryArgs {
    archive: String,
}

impl ToolSummaryLine for ArchiveListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let archive = self.archive.trim();
        if archive.is_empty() {
            return None;
        }
        Some(format!("list archive: {}", archive))
    }
}
