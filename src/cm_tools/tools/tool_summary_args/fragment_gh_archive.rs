// ── gh * ──────────────────────────────────────────────────────

fn gh_repo_suffix(repo: Option<String>) -> String {
    repo.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({})", r))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrChecksSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    number: Option<u64>,
}

impl ToolSummaryLine for GhPrChecksSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let suffix = gh_repo_suffix(self.repo);
        match self.number {
            Some(n) if n > 0 => Some(format!("gh pr checks #{}{}", n, suffix)),
            _ => Some(format!("gh pr checks{}", suffix)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrCreateSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    title: String,
    #[serde(default)]
    draft: Option<bool>,
}

impl ToolSummaryLine for GhPrCreateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let t = self.title.trim();
        if t.is_empty() {
            return None;
        }
        let mut head: String = t.chars().take(40).collect();
        if t.chars().count() > 40 {
            head.push('…');
        }
        let draft = self.draft == Some(true);
        Some(format!(
            "gh pr create{}{}{}",
            gh_repo_suffix(self.repo),
            if draft { " draft" } else { "" },
            if head.is_empty() {
                String::new()
            } else {
                format!(": {}", head)
            }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrMergeSummaryArgs {
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    merge_method: Option<String>,
}

impl ToolSummaryLine for GhPrMergeSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let method = self.merge_method.as_deref().unwrap_or("rebase");
        match self.number {
            Some(n) if n > 0 => Some(format!("gh pr merge #{n} ({method})")),
            _ => Some(format!("gh pr merge ({method})")),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrReviewSummaryArgs {
    event: String,
    #[serde(default)]
    number: Option<u64>,
}

impl ToolSummaryLine for GhPrReviewSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let ev = self.event.trim();
        if ev.is_empty() {
            return None;
        }
        match self.number {
            Some(n) if n > 0 => Some(format!("gh pr review #{n} {ev}")),
            _ => Some(format!("gh pr review {ev}")),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrCommentSummaryArgs {
    #[serde(default)]
    number: Option<u64>,
}

impl ToolSummaryLine for GhPrCommentSummaryArgs {
    fn summary_line(self) -> Option<String> {
        match self.number {
            Some(n) if n > 0 => Some(format!("gh pr comment #{n}")),
            _ => Some("gh pr comment".to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrBodyDraftSummaryArgs {
    #[serde(default)]
    base: Option<String>,
}

impl ToolSummaryLine for GhPrBodyDraftSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let base = self.base.as_deref().unwrap_or("main");
        Some(format!("gh pr body draft (base={base})"))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrEditSummaryArgs {
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    title: Option<String>,
}

impl ToolSummaryLine for GhPrEditSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let num = match self.number {
            Some(n) if n > 0 => format!("#{n}"),
            _ => String::new(),
        };
        let t = self.title.unwrap_or_default();
        let t = t.trim();
        if t.is_empty() {
            return Some(format!("gh pr edit {num}").trim_end().to_string());
        }
        let mut head: String = t.chars().take(40).collect();
        if t.chars().count() > 40 {
            head.push('…');
        }
        Some(format!("gh pr edit {num}: {head}").trim_end().to_string())
    }
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
pub(super) struct GhRunRerunSummaryArgs {
    run_id: String,
    #[serde(default)]
    failed: bool,
}

impl ToolSummaryLine for GhRunRerunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let id = self.run_id.trim();
        if id.is_empty() {
            return None;
        }
        Some(format!(
            "gh run rerun {}{}",
            id,
            if self.failed { " --failed" } else { "" }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhRunFailureSummarySummaryArgs {
    run_id: String,
}

impl ToolSummaryLine for GhRunFailureSummarySummaryArgs {
    fn summary_line(self) -> Option<String> {
        let id = self.run_id.trim();
        if id.is_empty() {
            return None;
        }
        Some(format!("gh run failure summary {id}"))
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

    #[test]
    fn ast_grep_rewrite_dry_run_defaults_true() {
        let v = json!({ "lang": "rust", "pattern": "foo" });
        let s = summarize_from_value::<AstGrepRewriteSummaryArgs>(&v).expect("summary");
        assert!(s.contains("(dry-run)"), "got {s}");
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FileExistsSummaryArgs {
    path: String,
}

impl ToolSummaryLine for FileExistsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("file exists: {}", path))
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
pub(super) struct SymlinkInfoSummaryArgs {
    path: String,
}

impl ToolSummaryLine for SymlinkInfoSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("symlink info: {}", path))
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
