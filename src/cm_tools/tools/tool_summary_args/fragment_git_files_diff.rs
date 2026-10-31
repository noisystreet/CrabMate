// ── git（action 选择器） ──────────────────────────────────────

/// `git` 的摘要行：一个结构体覆盖全部 action（原 10 个 `git_*` 工具中，
/// `git_diff` 用动态摘要、其余用静态文案；合并后统一在这里按 action 出文案）。
#[derive(Debug, Deserialize)]
pub(super) struct GitSummaryArgs {
    #[serde(default)]
    action: String,
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
    #[serde(default)]
    rev: Option<String>,
}

impl ToolSummaryLine for GitSummaryArgs {
    fn summary_line(self) -> Option<String> {
        match self.action.trim() {
            // `diff` 保留合并前的动态文案（flags/scope/path）。
            "diff" => {
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
            "show" => {
                let rev = self.rev.as_deref().unwrap_or("").trim();
                Some(if rev.is_empty() {
                    "git show HEAD".to_string()
                } else {
                    format!("git show {rev}")
                })
            }
            "blame" | "file_history" => {
                let path = self.path.as_deref().unwrap_or("").trim();
                let verb = if self.action.trim() == "blame" {
                    "git blame"
                } else {
                    "git file history"
                };
                Some(if path.is_empty() {
                    verb.to_string()
                } else {
                    format!("{verb} {path}")
                })
            }
            action => {
                // 固定文案的只读查询（与合并前的 `ToolSummaryKind::Static` 对齐）。
                let label = match action {
                    "status" => "git status",
                    "clean_check" => "git clean check",
                    "log" => "git log",
                    "branch_list" => "git branch",
                    "remote_status" => "git remote tracking status",
                    "remote_list" => "git remote -v",
                    // 缺 action 或未识别的 action 都回落到工具名，避免把原始字符串
                    // （如拼错的 `nope`）当成卡片标题显示。
                    _ => crate::cm_names::GIT,
                };
                Some(label.to_string())
            }
        }
    }
}
