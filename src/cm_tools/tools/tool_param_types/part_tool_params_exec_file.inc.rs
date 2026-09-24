// `exec_package` 与 `file_core` 手写 JSON Schema 迁移。

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSessionAction {
    Exec,
    SendSignal,
    Resize,
    List,
    Close,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TerminalSessionArgs {
    /// exec / send_signal / resize / list / close
    pub action: TerminalSessionAction,
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub input: Option<String>,
    pub signal: Option<i32>,
    #[schemars(range(min = 1))]
    pub cols: Option<u32>,
    #[schemars(range(min = 1))]
    pub rows: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunCommandArgs {
    /// 纯命令名（详见工具说明；禁止嵌入参数）
    pub command: String,
    pub args: Option<Vec<String>>,
    /// 墙钟秒（钳制 1～600，对齐 `python_snippet_run`）；`async` 与非 `async` 均生效。
    #[schemars(range(min = 1, max = 600))]
    pub timeout_secs: Option<u64>,
    /// `true` 时后台执行（默认 `false`）：创建后台任务、立即返回启动 `tool_result`
    /// （含 `tool_job_id` / `tool_job_poll_url` / `tool_job_status`），轮询/取消走后台任务端点。
    /// 需 `[tool_registry] background_jobs_enabled`（且 `run_command` 在 `background_job_async_tools` 白名单内）；
    /// 命令须已在白名单或已 AllowAlways 批准。
    #[serde(rename = "async")]
    pub async_: Option<bool>,
}

// ── file_core ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModifyFileMode {
    /// 整文件覆盖（`content` 为全文）；与 `Overwrite` 变体语义相同。
    Full,
    /// 与 `full` 相同，仅名称强调「整文件覆盖」语义。
    Overwrite,
    ReplaceLines,
    /// 在指定行后插入 `content`；`after_line=0` 表示插入文件开头。
    InsertAfterLine,
}

/// `edits` 内单条编辑的模式（不允许整文件覆盖）。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModifyFileEditMode {
    ReplaceLines,
    InsertAfterLine,
}

/// `edits` 内单条编辑：`start_line`/`end_line`（或仅 `end_line` 缺省时同 `start_line`）→ 行区间替换；
/// `after_line` → 锚点行后插入。行号均基于**调用时的磁盘快照**，由服务端自底向上应用，互不偏移。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModifyFileEditArgs {
    /// 省略时按字段推断：给了 `start_line`/`end_line` 即 `replace_lines`，给了 `after_line` 即 `insert_after_line`。
    pub mode: Option<ModifyFileEditMode>,
    /// 该条编辑写入的新内容；`replace_lines` 传 `""` 表示删除该区间。
    pub content: String,
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
    /// 0 表示文件开头，N 表示第 N 行之后。
    #[schemars(range(min = 0))]
    pub after_line: Option<u32>,
    /// `replace_lines` 守卫：写盘前校验该区间当前内容与此一致（按行精确比对），不一致即拒写并提示纠偏行号。
    pub expect_content: Option<String>,
    /// `insert_after_line` 守卫：写盘前校验锚点行（`after_line`）当前内容与此一致，不一致即拒写并提示纠偏行号。
    pub expect_line_content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModifyFileArgs {
    pub path: String,
    pub mode: Option<ModifyFileMode>,
    pub content: Option<String>,
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
    /// `mode=insert_after_line` 时使用；0 表示文件开头，N 表示第 N 行之后。
    #[schemars(range(min = 0))]
    pub after_line: Option<u32>,
    /// `mode=replace_lines` 守卫：写盘前校验 [start_line..=end_line] 当前内容与此一致（按行精确比对）；不一致即**拒写**并提示纠偏行号。同文件多次/并行编辑导致行号过期时，可避免误删误写。
    pub expect_content: Option<String>,
    /// `mode=insert_after_line` 守卫：写盘前校验锚点行（`after_line`）当前内容与此一致；不一致即拒写并提示纠偏行号。
    pub expect_line_content: Option<String>,
    /// 批量局部编辑：一次调用基于**同一磁盘快照**自底向上应用全部编辑，行号互不偏移；与 `mode`/`content`/`start_line`/`end_line`/`after_line` 互斥。
    pub edits: Option<Vec<ModifyFileEditArgs>>,
    /// 为 `true` 时只返回 unified diff 预览，**不写盘**（与 `search_replace` 的 `dry_run` 一致）。
    pub dry_run: Option<bool>,
    /// 当整文件覆盖被判定为高危（大幅缩短、大量删行、清空非空文件）时须显式 `true` 才执行写入。
    pub confirm_full_overwrite: Option<bool>,
    /// 为 `true` 时跳过写盘前语法校验（默认 `.py`/`.json`/`.toml` 会校验，失败拒写）。
    #[serde(default)]
    pub skip_precheck: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileFromToOverwriteArgs {
    pub from: String,
    pub to: String,
    pub overwrite: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ReadFileArgs {
    pub path: String,
    #[schemars(range(min = 1, max = 8000))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1, max = 8000))]
    pub end_line: Option<u32>,
    #[schemars(range(min = 1, max = 8000))]
    pub max_lines: Option<u32>,
    pub count_total_lines: Option<bool>,
    pub encoding: Option<String>,
    #[schemars(range(min = 1, max = 4000))]
    pub anchor_line: Option<u32>,
    #[schemars(range(min = 1, max = 4000))]
    pub context_lines: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobFilesArgs {
    pub pattern: String,
    pub path: Option<String>,
    #[schemars(range(min = 0, max = 100))]
    pub max_depth: Option<u32>,
    #[schemars(range(min = 1, max = 5000))]
    pub max_results: Option<u32>,
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListTreeArgs {
    pub path: Option<String>,
    #[schemars(range(min = 0, max = 60))]
    pub max_depth: Option<u32>,
    #[schemars(range(min = 1, max = 10000))]
    pub max_entries: Option<u32>,
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileExistsKind {
    File,
    Dir,
    Any,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileExistsArgs {
    pub path: String,
    pub kind: Option<FileExistsKind>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadBinaryMetaArgs {
    pub path: String,
    #[schemars(range(min = 0, max = 262144))]
    pub prefix_hash_bytes: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum HashFileAlgorithm {
    #[serde(rename = "sha256", alias = "sha-256")]
    Sha256,
    #[serde(rename = "sha512", alias = "sha-512")]
    Sha512,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HashFileArgs {
    pub path: String,
    pub algorithm: Option<HashFileAlgorithm>,
    /// 仅哈希前若干字节；整文件时省略
    #[schemars(range(min = 1, max = 4294967295u64))]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtractInFileMode {
    Lines,
    RustFnBlock,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ExtractInFileArgs {
    pub path: String,
    pub pattern: String,
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub max_matches: Option<u32>,
    pub case_insensitive: Option<bool>,
    #[schemars(range(min = 1))]
    pub max_snippet_chars: Option<u32>,
    pub mode: Option<ExtractInFileMode>,
    #[schemars(range(min = 1))]
    pub max_block_chars: Option<u32>,
    #[schemars(range(min = 1))]
    pub max_block_lines: Option<u32>,
    pub encoding: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct ApplyPatchArgs {
    pub patch: String,
    #[schemars(range(min = 0))]
    pub strip: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CodebaseSemanticSearchArgs {
    pub query: Option<String>,
    pub rebuild_index: Option<bool>,
    pub incremental: Option<bool>,
    pub path: Option<String>,
    #[schemars(range(min = 1, max = 64))]
    pub top_k: Option<u32>,
    #[schemars(range(min = 0, max = 2_000_000))]
    pub query_max_chunks: Option<u32>,
    pub file_glob: Option<String>,
    pub extensions: Option<Vec<String>>,
    pub retrieve_mode: Option<String>,
    pub hybrid_alpha: Option<f64>,
    #[schemars(range(min = 1, max = 10_000))]
    pub fts_top_n: Option<u32>,
    #[schemars(range(min = 1, max = 10_000))]
    pub hybrid_semantic_pool: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct SearchInFilesEnhancedArgs {
    pub pattern: String,
    pub path: Option<String>,
    #[schemars(range(min = 1))]
    pub max_results: Option<u32>,
    pub case_insensitive: Option<bool>,
    pub ignore_hidden: Option<bool>,
    #[schemars(range(min = 0, max = 10))]
    pub context_before: Option<u32>,
    #[schemars(range(min = 0, max = 10))]
    pub context_after: Option<u32>,
    pub file_glob: Option<String>,
    pub exclude_glob: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReadDirSortBy {
    Name,
    Size,
    Mtime,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct ReadDirEnhancedArgs {
    pub path: Option<String>,
    #[schemars(range(min = 1))]
    pub max_entries: Option<u32>,
    pub include_hidden: Option<bool>,
    pub include_size: Option<bool>,
    pub include_mtime: Option<bool>,
    pub sort_by: Option<ReadDirSortBy>,
}
