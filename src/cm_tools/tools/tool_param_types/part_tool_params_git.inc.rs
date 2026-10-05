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
    /// 可选：仅输出统计信息（等价 git diff --stat），默认 false
    pub stat: Option<bool>,
    /// 可选：仅输出变更文件名（等价 git diff --name-only），默认 false
    pub name_only: Option<bool>,
    /// 可选：基准分支；设置后改为对比 base...HEAD（忽略 mode），如 "main"
    pub base: Option<String>,
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
