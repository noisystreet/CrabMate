// `git` 的入参：单一 `action` 选择器 + 各只读子命令的字段并集（见下方 `GitArgs`）。

// ── git read ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitDiffMode {
    #[default]
    Working,
    Staged,
    All,
}

/// `git` 入参：`action` 选择器 + 各只读查询的字段并集（按 action 取用，多余字段被忽略）。
///
/// 合并自原 10 个 `git_*` 工具——它们都是同一个 `git` CLI 的只读子命令包装，
/// 原先 schema 把 `path` / `max_count` / `porcelain` 等字段在各工具里重复声明。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GitArgs {
    /// 要执行的只读 Git 查询。
    pub action: GitAction,
    /// `status`：机器可读的 `--porcelain` 输出（默认 false）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub porcelain: Option<bool>,
    /// `status`：是否显示未跟踪文件（默认 true）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_untracked: Option<bool>,
    /// `status`：是否显示分支信息（默认 true）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<bool>,
    /// `diff`：working（未暂存）/ staged（已暂存）/ all（两者都看），默认 working。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<GitDiffMode>,
    /// `diff` / `blame` / `file_history`：工作区内相对路径（`blame` 与 `file_history` 必填）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `diff`：每处变更的上下文行数（-U），默认 3。
    #[schemars(range(min = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_lines: Option<u32>,
    /// `diff`：仅输出统计信息（--stat）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat: Option<bool>,
    /// `diff`：仅输出变更文件名（--name-only）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_only: Option<bool>,
    /// `diff`：设置后改为对比 `base...HEAD`（忽略 mode）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// `log`（默认 20）/ `file_history`（默认 30）：最多返回条数。
    #[schemars(range(min = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_count: Option<u32>,
    /// `log`：是否使用单行展示（默认 true）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oneline: Option<bool>,
    /// `show`：提交号/引用（默认 HEAD）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// `blame`：起始行（需与 `end_line` 一起使用）。
    #[schemars(range(min = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    /// `blame`：结束行（需与 `start_line` 一起使用）。
    #[schemars(range(min = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// `branch_list`：是否包含远程分支（默认 true）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_remote: Option<bool>,
}

/// `git` 的 action 取值；与 `git::read_ops` 各查询函数一一对应。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitAction {
    /// `git status`
    Status,
    /// `git diff`
    Diff,
    /// `git status --porcelain`（是否干净）
    CleanCheck,
    /// `git log`
    Log,
    /// `git show`
    Show,
    /// `git blame`
    Blame,
    /// `git log --follow`（单文件历史）
    FileHistory,
    /// `git branch`
    BranchList,
    /// `git status -sb`（远程跟踪关系）
    RemoteStatus,
    /// `git remote -v`
    RemoteList,
}
