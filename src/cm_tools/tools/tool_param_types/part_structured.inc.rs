// ── 结构化数据（`structured_data`）────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum StructuredDataFormat {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "yaml")]
    Yaml,
    #[serde(rename = "yml")]
    Yml,
    #[serde(rename = "toml")]
    Toml,
    #[serde(rename = "csv")]
    Csv,
    #[serde(rename = "tsv")]
    Tsv,
}

impl StructuredDataFormat {
    pub fn as_detect_token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Yml => "yml",
            Self::Toml => "toml",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum StructuredPatchFormat {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "yaml")]
    Yaml,
    #[serde(rename = "yml")]
    Yml,
    #[serde(rename = "toml")]
    Toml,
}

impl StructuredPatchFormat {
    pub fn as_detect_token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Yml => "yml",
            Self::Toml => "toml",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StructuredPatchAction {
    Set,
    Remove,
}

fn default_structured_patch_action() -> StructuredPatchAction {
    StructuredPatchAction::Set
}

/// [`super::structured_data::structured_validate_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredValidateArgs {
    pub path: String,
    pub format: Option<StructuredDataFormat>,
    #[serde(default = "default_true")]
    pub has_header: bool,
    #[serde(default = "default_true")]
    pub summarize: bool,
}

/// [`super::structured_data::structured_query_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredQueryArgs {
    pub path: String,
    pub query: String,
    pub format: Option<StructuredDataFormat>,
    #[serde(default = "default_true")]
    pub has_header: bool,
}

fn default_structured_diff_max_lines() -> Option<u64> {
    Some(200)
}

/// [`super::structured_data::structured_diff_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredDiffArgs {
    pub path_a: String,
    pub path_b: String,
    pub format: Option<StructuredDataFormat>,
    #[serde(default = "default_true")]
    pub has_header: bool,
    #[serde(default = "default_structured_diff_max_lines")]
    #[schemars(range(min = 1, max = 2000))]
    pub max_diff_lines: Option<u64>,
}

/// [`super::structured_data::structured_patch_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredPatchArgs {
    pub path: String,
    pub query: String,
    #[serde(default = "default_structured_patch_action")]
    pub action: StructuredPatchAction,
    pub value: Option<JsonValue>,
    pub format: Option<StructuredPatchFormat>,
    #[serde(default = "default_true")]
    pub create_missing: bool,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// **已废弃**：写盘只需 `dry_run=false`，本字段不再被校验；仅为兼容既有调用保留。
    #[serde(default)]
    pub confirm: bool,
    /// 写前校验：`read_file` 输出头里的 `revision`（`sha256:<hex>`）。
    /// 与目标文件当前内容不一致即拒写（`错误[stale_revision]`），避免覆盖别人刚改过的文件。
    /// 不传则不做 CAS，保持「按 key 路径改」的既有行为（键路径不受行号漂移影响，
    /// 故不像行号类编辑那样强制要求基线）。
    ///
    /// **此字段仅用于 schema 声明与文档**：实际取值由写侧守卫
    /// （`file::verify_write_revision`）从**原始入参 JSON** 读取，与 `modify_file` 同模式。
    /// 改校验语义请改那里，不要以为改这里的类型就能生效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
}

// ── `structured` 单入口（四个子动作合并为一个工具）─────────────

/// `structured` 的 `action` 选择器：四个子动作合并成一个工具名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StructuredAction {
    Validate,
    Query,
    Diff,
    Patch,
}

/// [`super::structured_dispatch::structured_try`] 入参。
///
/// 字段是四个子动作的**并集**：只有本 `action` 用到的键会被转发给子实现，
/// 其余键静默忽略（与 `code_nav` 同一约定）。必填项按 `action` 在分发层校验。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredArgs {
    /// **必填**：`validate` / `query` / `diff` / `patch`。
    pub action: StructuredAction,
    /// `validate` / `query` / `patch`：目标文件（工作区相对路径）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `diff`：左侧文件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_a: Option<String>,
    /// `diff`：右侧文件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_b: Option<String>,
    /// `query` / `patch`：JSON Pointer（`/a/b`）或点号路径（`a.b.0`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// `patch`：`set`（默认，需同时给 `value`）或 `remove`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<StructuredPatchAction>,
    /// `patch` + `mutation=set`：写入的新值（任意 JSON）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
    /// 显式格式（`auto` / `json` / `yaml` / `yml` / `toml` / `csv` / `tsv`）；省略按扩展名推断。
    /// `patch` 不接受 `csv` / `tsv`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<StructuredDataFormat>,
    /// `validate` / `query` / `diff`：CSV/TSV 首行是否为表头（默认 `true`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_header: Option<bool>,
    /// `validate`：是否输出顶层摘要（默认 `true`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarize: Option<bool>,
    /// `diff`：最多输出多少条差异（默认 200，上限 2000）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 2000))]
    pub max_diff_lines: Option<u64>,
    /// `patch`：父级缺失时是否创建（默认 `true`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_missing: Option<bool>,
    /// `patch`：**默认 `true` 仅预览**；显式 `false` 才写盘。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// `patch`：**已废弃**（仅为兼容保留，不再校验）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<bool>,
    /// `patch`：写前 CAS 基线（`read_file` 输出头里的 `revision`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
}

// ── 工作流执行（`workflow_execute` 占位 runner）───────────────

/// [`super::runners::runner_workflow_execute`] 对应 parameters（DAG 由运行时解析）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecuteArgs {
    /// 内联 DAG / `steps` / `workflow_template`（与 `workflow_file` 二选一或叠加覆盖）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<JsonValue>,
    /// 工作区相对路径：`.yaml` / `.yml`，或含 `` ```crabmate-workflow `` 的 `.md`（如 `examples/workflows/ci.yaml` 或工作区内 `.crabmate/workflows/ci.yaml`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_file: Option<String>,
}

// ── 拼写（`spell_tools`，供 `docs_health_sweep` 复用）────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TyposCheckArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub config_path: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CodespellCheckArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub skip: Option<String>,
    #[serde(default)]
    pub dictionary_paths: Vec<String>,
    pub ignore_words_list: Option<String>,
}

// ── `docs_health_sweep` ───────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DocsHealthSweepArgs {
    #[serde(default = "default_true")]
    pub run_doc_preview: bool,
    #[serde(default)]
    pub doc_paths: Vec<String>,
    #[serde(default = "default_doc_preview_max_lines")]
    #[schemars(range(min = 10, max = 200))]
    pub doc_preview_max_lines: Option<u64>,
    #[serde(default = "default_true")]
    pub run_typos: bool,
    #[serde(default = "default_true")]
    pub run_codespell: bool,
    #[serde(default = "default_true")]
    pub run_markdown_links: bool,
    #[serde(default)]
    pub spell_paths: Vec<String>,
    pub typos_config_path: Option<String>,
    pub codespell_skip: Option<String>,
    #[serde(default)]
    pub codespell_dictionary_paths: Vec<String>,
    pub codespell_ignore_words_list: Option<String>,
    #[serde(default)]
    pub md_roots: Vec<String>,
    pub md_max_files: Option<u64>,
    pub md_max_depth: Option<u64>,
    #[serde(default)]
    pub md_allowed_external_prefixes: Vec<String>,
    pub md_external_timeout_secs: Option<u64>,
    pub md_check_fragments: Option<bool>,
    pub md_output_format: Option<MarkdownCheckLinksOutputFormat>,
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default)]
    pub summary_only: bool,
}

fn default_doc_preview_max_lines() -> Option<u64> {
    Some(60)
}

// ── `table_text` / `text_diff` ─────────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TableTextAction {
    Preview,
    Validate,
    SelectColumns,
    FilterRows,
    Aggregate,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum TableTextDelimiter {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "comma")]
    Comma,
    #[serde(rename = "csv")]
    Csv,
    #[serde(rename = "tab")]
    Tab,
    #[serde(rename = "tsv")]
    Tsv,
    #[serde(rename = "semicolon")]
    Semicolon,
    #[serde(rename = "pipe")]
    Pipe,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TableTextAggregateOp {
    Count,
    CountNonEmpty,
    CountNumeric,
    Sum,
    Mean,
    #[serde(rename = "avg")]
    Avg,
    Min,
    Max,
}

/// [`super::table_text::run_try`] 入参（按 `action` 分支校验由 runner 完成）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableTextArgs {
    pub action: TableTextAction,
    pub path: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub delimiter: Option<TableTextDelimiter>,
    #[serde(default = "default_true")]
    pub has_header: bool,
    #[serde(default)]
    #[schemars(range(min = 1, max = 200))]
    pub preview_rows: Option<u64>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 200_000))]
    pub max_rows_scan: Option<u64>,
    #[serde(default)]
    pub columns: Vec<u64>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub column: Option<u64>,
    pub equals: Option<String>,
    pub contains: Option<String>,
    pub op: Option<TableTextAggregateOp>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 10_000))]
    pub max_output_rows: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextDiffMode {
    Inline,
    Paths,
}

fn default_text_diff_mode() -> TextDiffMode {
    TextDiffMode::Inline
}

fn default_text_diff_context_lines() -> Option<u64> {
    Some(3)
}

fn default_text_diff_max_output_bytes() -> Option<u64> {
    Some(50_000)
}

/// [`super::text_diff::run_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TextDiffArgs {
    #[serde(default = "default_text_diff_mode")]
    pub mode: TextDiffMode,
    pub left: Option<String>,
    pub right: Option<String>,
    pub left_path: Option<String>,
    pub right_path: Option<String>,
    #[serde(default = "default_text_diff_context_lines")]
    #[schemars(range(min = 0, max = 20))]
    pub context_lines: Option<u64>,
    #[serde(default = "default_text_diff_max_output_bytes")]
    #[schemars(range(min = 1, max = 500_000))]
    pub max_output_bytes: Option<u64>,
}

impl Default for TextDiffArgs {
    fn default() -> Self {
        Self {
            mode: default_text_diff_mode(),
            left: None,
            right: None,
            left_path: None,
            right_path: None,
            context_lines: default_text_diff_context_lines(),
            max_output_bytes: default_text_diff_max_output_bytes(),
        }
    }
}

// ── 文件额外写操作（`file::mutate` / `perm` / `symlink`）────────

/// [`super::file::mutate::delete_files`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteFilesArgs {
    /// 待删除的相对路径列表（1..=32；同路径去重；不能是目录）。
    pub paths: Vec<String>,
    /// 破坏性操作确认门：必须为 `true`。
    pub confirm: Option<bool>,
}

/// [`super::file::mutate::delete_dir`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteDirArgs {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    pub confirm: Option<bool>,
}

/// [`super::file::mutate::append_file`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AppendFileArgs {
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub create_if_missing: bool,
    /// **默认 `true`**：只返回 unified diff 预览、**不写盘**；显式 `false` 才写入。
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// 原文件非空且末尾无换行时，追加前自动补一个 `\n`，避免把新内容接在旧末行后。
    #[serde(default = "default_true")]
    pub ensure_leading_newline: bool,
    /// 为 `true` 时跳过写盘前语法校验（默认 `.py`/`.json`/`.toml` 会校验，失败拒写）。
    #[serde(default)]
    pub skip_precheck: bool,
}

/// [`super::file::mutate::create_dir`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateDirArgs {
    pub path: String,
    #[serde(default = "default_true")]
    pub parents: bool,
}

/// [`super::file::mutate::search_replace`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchReplaceArgs {
    pub path: String,
    pub search: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub max_replacements: Option<u64>,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// **已废弃**：写盘只需 `dry_run=false`，本字段不再被校验；仅为兼容既有调用保留。
    #[serde(default)]
    pub confirm: bool,
    /// 为 `true` 时跳过写盘前语法校验（默认 `.py`/`.json`/`.toml` 会校验，失败拒写）。
    #[serde(default)]
    pub skip_precheck: bool,
}

