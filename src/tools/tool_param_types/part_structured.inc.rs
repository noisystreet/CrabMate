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
    pub(crate) fn as_detect_token(self) -> &'static str {
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
    pub(crate) fn as_detect_token(self) -> &'static str {
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

/// [`super::structured_data::structured_validate`] 入参。
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

/// [`super::structured_data::structured_query`] 入参。
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

/// [`super::structured_data::structured_diff`] 入参。
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

/// [`super::structured_data::structured_patch`] 入参。
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
    #[serde(default)]
    pub confirm: bool,
}

// ── 工作流执行（`workflow_execute` 占位 runner）───────────────

/// [`super::runners::runner_workflow_execute`] 对应 parameters（DAG 由运行时解析）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecuteArgs {
    pub workflow: JsonValue,
}

// ── 调试（`debug_tools::rust_backtrace_analyze`）──────────────

/// [`super::debug_tools::rust_backtrace_analyze`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BacktraceAnalyzeArgs {
    pub backtrace: String,
    pub crate_hint: Option<String>,
}

// ── 本地 CI / 发版检查（`ci_tools`）──────────────────────────

/// [`super::ci_tools::ci_pipeline_local`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CiPipelineLocalArgs {
    #[serde(default = "default_true")]
    pub run_fmt: bool,
    #[serde(default = "default_true")]
    pub run_clippy: bool,
    #[serde(default = "default_true")]
    pub run_test: bool,
    #[serde(default = "default_true")]
    pub run_frontend_lint: bool,
    #[serde(default)]
    pub run_frontend_build: bool,
    #[serde(default = "default_true")]
    pub run_ruff_check: bool,
    #[serde(default)]
    pub run_pytest: bool,
    #[serde(default)]
    pub run_mypy: bool,
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default)]
    pub summary_only: bool,
}

/// [`super::ci_tools::release_ready_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ReleaseReadyCheckArgs {
    #[serde(default = "default_true")]
    pub run_ci: bool,
    #[serde(default = "default_true")]
    pub run_audit: bool,
    #[serde(default = "default_true")]
    pub run_deny: bool,
    #[serde(default = "default_true")]
    pub require_clean_worktree: bool,
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default = "default_true")]
    pub summary_only: bool,
}

// ── `pre_commit_run` ──────────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct PreCommitRunArgs {
    pub hook: Option<String>,
    #[serde(default)]
    pub all_files: bool,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub verbose: bool,
}

// ── 拼写 / ast-grep（`spell_astgrep_tools`）────────────────────

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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AstGrepRunArgs {
    pub pattern: String,
    pub lang: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub globs: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AstGrepRewriteArgs {
    pub pattern: String,
    pub rewrite: String,
    pub lang: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default)]
    pub confirm: bool,
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

/// [`super::table_text::run`] 入参（按 `action` 分支校验由 runner 完成）。
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

/// [`super::text_diff::run`] 入参。
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

/// [`super::file::mutate::delete_file`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
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
    #[serde(default)]
    pub confirm: bool,
}

/// [`super::file::perm::chmod_file`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ChmodFileArgs {
    pub path: String,
    pub mode: String,
    pub confirm: Option<bool>,
}

/// [`super::file::symlink::symlink_info`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SymlinkInfoArgs {
    pub path: String,
}
