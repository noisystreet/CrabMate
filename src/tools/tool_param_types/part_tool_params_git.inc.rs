// 原 `tool_params/{git_read,git_write}.rs` 手写 JSON；与 `git` runner 的 Value 解析形状对齐。

/// 无参工具（如 `git_remote_list`）的 JSON 对象。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct EmptyToolArgs {}

// ── git read ────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitStatusArgs {
    /// 可选：是否使用机器可读的 --porcelain 输出，默认 false
    pub porcelain: Option<bool>,
    /// 可选：是否显示未跟踪文件，默认 true
    pub include_untracked: Option<bool>,
    /// 可选：是否显示分支信息，默认 true
    pub branch: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitDiffMode {
    #[default]
    Working,
    Staged,
    All,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitDiffArgs {
    /// diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。
    pub mode: Option<GitDiffMode>,
    /// 可选：仅查看某个相对路径（文件或目录）的 diff，如 src/main.rs
    pub path: Option<String>,
    /// 可选：每处变更展示上下文行数（-U），默认 3
    #[schemars(range(min = 0))]
    pub context_lines: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitDiffStatArgs {
    pub mode: Option<GitDiffMode>,
    pub path: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitDiffNamesArgs {
    pub mode: Option<GitDiffMode>,
    pub path: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitLogArgs {
    /// 可选：最多返回提交条数，默认 20
    #[schemars(range(min = 1))]
    pub max_count: Option<u32>,
    /// 可选：是否使用单行展示，默认 true
    pub oneline: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitShowArgs {
    /// 可选：提交号/引用，默认 HEAD
    pub rev: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitDiffBaseArgs {
    /// 可选：基准分支，默认 main（对比 base...HEAD）
    pub base: Option<String>,
    #[schemars(range(min = 0))]
    pub context_lines: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitBlameArgs {
    /// 相对路径（必填）
    pub path: String,
    /// 可选：起始行（需和 end_line 一起使用）
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    /// 可选：结束行（需和 start_line 一起使用）
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitFileHistoryArgs {
    /// 相对路径（必填）
    pub path: String,
    /// 可选：最多返回提交条数，默认 30
    #[schemars(range(min = 1))]
    pub max_count: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitBranchListArgs {
    /// 可选：是否包含远程分支，默认 true
    pub include_remote: Option<bool>,
}

// ── git write ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitStageFilesArgs {
    /// 要暂存的相对路径列表（必填）
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitCommitArgs {
    /// 提交信息（必填）
    pub message: String,
    /// 可选：提交前是否执行 git add -A，默认 false
    pub stage_all: Option<bool>,
    /// 安全确认；仅当 true 时才会执行 commit
    pub confirm: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitFetchArgs {
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub prune: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitRemoteSetUrlArgs {
    pub name: String,
    pub url: String,
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitApplyArgs {
    pub patch_path: String,
    pub check_only: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitCloneArgs {
    pub repo_url: String,
    pub target_dir: String,
    #[schemars(range(min = 1))]
    pub depth: Option<u32>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitCheckoutArgs {
    pub target: String,
    pub create: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitBranchCreateArgs {
    pub name: String,
    pub start_point: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitBranchDeleteArgs {
    pub name: String,
    pub force: Option<bool>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitPushArgs {
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub set_upstream: Option<bool>,
    pub force_with_lease: Option<bool>,
    pub tags: Option<bool>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitMergeArgs {
    pub branch: String,
    pub no_ff: Option<bool>,
    pub squash: Option<bool>,
    pub message: Option<String>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitRebaseArgs {
    pub onto: Option<String>,
    pub abort: Option<bool>,
    #[serde(rename = "continue")]
    pub continue_rebase: Option<bool>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitStashAction {
    Push,
    Pop,
    Apply,
    List,
    Drop,
    Clear,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitStashArgs {
    pub action: Option<GitStashAction>,
    pub message: Option<String>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitTagAction {
    List,
    Create,
    Delete,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitTagArgs {
    pub action: Option<GitTagAction>,
    pub name: Option<String>,
    pub message: Option<String>,
    pub pattern: Option<String>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitResetArgs {
    pub mode: Option<GitResetMode>,
    pub target: Option<String>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct GitCherryPickArgs {
    pub commit: Option<String>,
    pub commits: Option<Vec<String>>,
    pub no_commit: Option<bool>,
    pub abort: Option<bool>,
    #[serde(rename = "continue")]
    pub continue_pick: Option<bool>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitRevertArgs {
    pub commit: Option<String>,
    pub no_commit: Option<bool>,
    pub abort: Option<bool>,
    #[serde(rename = "continue")]
    pub continue_revert: Option<bool>,
    pub confirm: Option<bool>,
}
