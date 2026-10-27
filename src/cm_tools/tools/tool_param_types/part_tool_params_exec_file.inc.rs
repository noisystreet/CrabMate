// `exec_package` 与 `file_core` 手写 JSON Schema 迁移。

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSessionAction {
    Exec,
    SendSignal,
    Interrupt,
    Resize,
    List,
    Close,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TerminalSessionArgs {
    /// exec / send_signal / interrupt / resize / list / close
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
    /// 纯命令名（详见工具说明；禁止嵌入参数）。与 `script` 二选一。
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    /// 工作区内的相对目录（等价于 `cd <cwd> && ...`）；禁止绝对路径与 `..`。
    pub cwd: Option<String>,
    /// 交给 POSIX shell（`bash -c` / `sh -c`）执行的脚本；与 `command`/`args` 互斥。
    pub script: Option<String>,
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

/// 局部编辑的内容守卫：`modify_file` 顶层与 `edits[i]` 共用同一类型
/// （原先四个 `expect_*` 平铺字段在两处各定义一遍）。
///
/// - `replace_lines`：`content`（整段逐行比对）或 `first_line`+`last_line`（只锚首尾）；两者互斥。
/// - `insert_after_line`：`anchor_line`（比对 `after_line` 那一行）。
///
/// 键名与旧平铺参数一一对应，旧写法仍被接受，但 schema / description 只暴露本结构。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteGuard {
    /// `replace_lines`：逐行校验 `[start_line..=end_line]`；与 `first_line`/`last_line` 互斥。
    pub content: Option<String>,
    /// `replace_lines`：只校验区间首行（`start_line`）。
    pub first_line: Option<String>,
    /// `replace_lines`：只校验区间末行（`end_line`）。
    pub last_line: Option<String>,
    /// `insert_after_line`：校验插入锚点行（`after_line`）。
    pub anchor_line: Option<String>,
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
    /// 内容守卫：见 [`WriteGuard`]。
    pub guard: Option<WriteGuard>,
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
    /// 内容守卫：见 [`WriteGuard`]（`replace_lines` 用 `content` 或 `first_line`+`last_line`，
    /// `insert_after_line` 用 `anchor_line`）。与 `mode=full/overwrite` 互斥。
    pub guard: Option<WriteGuard>,
    /// 批量局部编辑：一次调用基于**同一磁盘快照**自底向上应用全部编辑，行号互不偏移；与 `mode`/`content`/`start_line`/`end_line`/`after_line` 互斥。
    pub edits: Option<Vec<ModifyFileEditArgs>>,
    /// **默认 `true`**：只返回 unified diff 预览、**不写盘**；显式 `false` 才写入（与 `search_replace` / `structured` 一致）。
    pub dry_run: Option<bool>,
    /// 可选写前校验：最近一次 `read_file` 成功输出头中的 `revision`。与当前磁盘内容不一致时**拒绝写入**
    /// （`错误[stale_revision]`），避免基于过期快照按已漂移的行号误改；适用于所有 `mode` 与 `edits`。
    /// 显式传入时**优先**于本轮自动绑定，且始终校验。
    pub base_revision: Option<String>,
    /// 行号类模式（`replace_lines` / `insert_after_line` / `edits`）默认使用本轮最近一次 `read_file`
    /// 的 revision 自动校验；设为 `true` 可跳过该自动绑定（显式 `base_revision` 仍会校验）。
    pub allow_stale_revision: Option<bool>,
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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadBinaryMetaArgs {
    pub path: String,
    #[schemars(range(min = 0, max = 262144))]
    pub prefix_hash_bytes: Option<u32>,
}


#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct ApplyPatchArgs {
    pub patch: String,
    #[schemars(range(min = 0))]
    pub strip: Option<u32>,
    /// **默认 `false`**：保持 `apply_patch`「就是应用」的既有语义。设为 `true` 时只试算并返回
    /// diff 预览、不落盘。
    ///
    /// 与 `dry_run` 正交的一点：多文件补丁**总是**先整体试算，任一文件对不上就整体不写，
    /// 不会出现「部分应用」。
    pub dry_run: bool,
    /// 写前校验（**仅限单文件补丁**）：`read_file` 输出头里的 `revision`。
    /// 与目标当前内容不一致即拒写（`错误[stale_revision]`）。补丁涉及多个文件时传此参数会
    /// 直接报错——一个 revision 无法表达多个文件的基线。
    ///
    /// **此字段仅用于 schema 声明与文档**：取值由写侧守卫（`file::verify_write_revision`）
    /// 从**原始入参 JSON** 读取，与 `modify_file` / `structured`（`action=patch`）同模式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
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
