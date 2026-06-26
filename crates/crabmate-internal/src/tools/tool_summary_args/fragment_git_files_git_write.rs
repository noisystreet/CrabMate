// ── Git write summaries ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct GitCheckoutSummaryArgs {
    target: String,
    #[serde(default)]
    create: bool,
}

impl ToolSummaryLine for GitCheckoutSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let target = self.target.trim();
        if target.is_empty() {
            return None;
        }
        if self.create {
            Some(format!("git checkout -b {}", target))
        } else {
            Some(format!("git checkout {}", target))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitBranchCreateSummaryArgs {
    name: String,
}

impl ToolSummaryLine for GitBranchCreateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        Some(format!("git branch create: {}", name))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitBranchDeleteSummaryArgs {
    name: String,
}

impl ToolSummaryLine for GitBranchDeleteSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        Some(format!("git branch delete: {}", name))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitPushSummaryArgs {
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    branch: Option<String>,
}

impl ToolSummaryLine for GitPushSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let remote = self.remote.as_deref().unwrap_or("origin");
        let branch = self.branch.as_deref().unwrap_or("").trim();
        if branch.is_empty() {
            Some(format!("git push {}", remote))
        } else {
            Some(format!("git push {} {}", remote, branch))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitMergeSummaryArgs {
    branch: String,
}

impl ToolSummaryLine for GitMergeSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let branch = self.branch.trim();
        if branch.is_empty() {
            return None;
        }
        Some(format!("git merge {}", branch))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitRebaseSummaryArgs {
    #[serde(default)]
    abort: bool,
    #[serde(default, rename = "continue")]
    continue_rebase: bool,
    #[serde(default)]
    onto: Option<String>,
}

impl ToolSummaryLine for GitRebaseSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.abort {
            return Some("git rebase --abort".to_string());
        }
        if self.continue_rebase {
            return Some("git rebase --continue".to_string());
        }
        let onto = self.onto.as_deref().unwrap_or("?");
        Some(format!("git rebase onto {}", onto))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitStashSummaryArgs {
    #[serde(default)]
    action: Option<String>,
}

impl ToolSummaryLine for GitStashSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.as_deref().unwrap_or("push");
        Some(format!("git stash {}", action))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitTagSummaryArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ToolSummaryLine for GitTagSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.as_deref().unwrap_or("list");
        match action {
            "create" => {
                let name = self.name.as_deref().unwrap_or("?");
                Some(format!("git tag create: {}", name))
            }
            "delete" => {
                let name = self.name.as_deref().unwrap_or("?");
                Some(format!("git tag delete: {}", name))
            }
            _ => Some("git tag list".to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitResetSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

impl ToolSummaryLine for GitResetSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("mixed");
        let target = self.target.as_deref().unwrap_or("HEAD");
        Some(format!("git reset --{} {}", mode, target))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitRevertSummaryArgs {
    #[serde(default)]
    abort: bool,
    #[serde(default)]
    commit: Option<String>,
}

impl ToolSummaryLine for GitRevertSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.abort {
            return Some("git revert --abort".to_string());
        }
        let commit = self.commit.as_deref().unwrap_or("?");
        Some(format!("git revert {}", commit))
    }
}

