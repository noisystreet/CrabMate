/// [`super::weather::run_try`] 入参（`city` 与 `location` 二选一，至少 2 字符由 runner 校验）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GetWeatherArgs {
    pub city: Option<String>,
    pub location: Option<String>,
}

/// [`super::web_search::run_try`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebSearchArgs {
    pub query: String,
    /// 1～20；省略时用配置默认
    pub max_results: Option<u64>,
    /// 本次调用的 worbrow 代理（`http://host:port` / `https://host:port`）；
    /// 省略/空串时回退配置 `web_search_proxy`。仅 `worbrow` 生效；不接受 `socks5://` 与凭据。
    pub proxy: Option<String>,
}


/// [`super::regex_test::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegexTestArgs {
    pub pattern: String,
    pub test_strings: Vec<String>,
}

/// [`super::date_calc::run`] 的 `mode`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DateCalcMode {
    #[default]
    Offset,
    Diff,
}

/// [`super::date_calc::run`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DateCalcArgs {
    #[serde(default)]
    pub mode: Option<DateCalcMode>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub base: Option<String>,
    pub offset: Option<String>,
}

/// [`super::json_format::run`] 的 `mode`。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JsonFormatMode {
    Pretty,
    Compact,
    YamlToJson,
    JsonToYaml,
}

/// [`super::json_format::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonFormatArgs {
    pub text: String,
    #[serde(default)]
    pub mode: Option<JsonFormatMode>,
}

/// [`super::env_var_check::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvVarCheckArgs {
    pub names: Vec<String>,
    #[serde(default)]
    pub show_length: Option<bool>,
    pub show_prefix_chars: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// `background_job` 的 action 取值；与 `background_job_tools` 的三个只读查询一一对应。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundJobAction {
    /// 按 `tool_job_id` 查任务状态与（终态）输出。
    Status,
    /// 列出当前工作区的后台任务（最新创建在前）。
    List,
    /// 增量拉取任务输出（`tail -f` 语义）。
    Output,
}

/// [`super::background_job_tools::background_job`] 入参：`action` 选择器 + 字段并集。
///
/// 合并自原 `background_job_status` / `background_job_list` / `background_job_output` 三个只读
/// 工具。`background_job_cancel` 会改变任务状态（写副作用），**保持独立工具名**，避免把这三个
/// 只读查询一并拖出并行只读批 / 只读重试 / `Ask`·`Plan` 门控 / TTL 缓存
/// （见 `registry_policy::builtin_write_effect_tools`）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackgroundJobArgs {
    /// 要执行的后台任务查询（只读）。
    pub action: BackgroundJobAction,
    /// `status` / `output`：后台任务 id（`run_command` 以 `async: true` 启动时返回，形如 `tooljob_...`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_job_id: Option<String>,
    /// `output`：增量游标（上次返回的 `next_cursor`）；省略/非法则从最早可用起
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// `list`：最多返回条数，默认 20，上限 100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u32>,
}

/// [`super::background_job_tools::background_job_cancel`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackgroundJobCancelArgs {
    /// 后台任务 id（`run_command` 以 `async: true` 启动时返回，形如 `tooljob_...`）
    pub tool_job_id: String,
}

/// `background_job_wait` 的 `until` 取值。
///
/// 只取保守的 A 案（#1194 Q1）：`exit` 与 `output_matches`。**不做** `idle_secs` ——
/// `terminal_session` 的 `IDLE_DRAIN` 之所以成立，是因为 PTY 没有「结束」信号；
/// 普通命令有 exit code，引入「输出暂停」等于用一个更弱的信号覆盖更强的信号，
/// 卡死会被误判成「暂时没输出」。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundJobWaitUntil {
    /// 等到任务终态（`succeeded` / `failed` / `cancelled` / `timed_out`）。
    #[default]
    Exit,
    /// 保留窗口内出现 `output_matches` 子串即返回（不必等到结束）。
    OutputMatches,
}

/// [`super::background_job_tools::background_job_wait_try`] 入参。
///
/// **独立工具**，不并入 `background_job`：阻塞等待会同时破坏该工具的
/// 并行只读批身份与同轮 `(name, args)` 去重缓存（见 `docs/design/background_tool_jobs_contract.md` §9）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackgroundJobWaitArgs {
    /// 后台任务 id（`run_command` 以 `async: true` 启动时返回，形如 `tooljob_...`）
    pub tool_job_id: String,
    /// 等待条件，默认 `exit`（跑到终态）。
    #[serde(default)]
    pub until: Option<BackgroundJobWaitUntil>,
    /// `until=output_matches` 时必填：在任务输出（stdout/stderr 合并、保留窗口内）中查找的**子串**（非正则，≤ 256 字节）。
    #[serde(default)]
    pub output_matches: Option<String>,
    /// 本次等待的墙钟上限（秒），钳制 1–600，默认 60；**不会**取消任务。
    #[serde(default)]
    #[schemars(range(min = 1, max = 600))]
    pub timeout_secs: Option<u64>,
}

/// [`super::markdown_links::markdown_check_links_try`] 的 `output_format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MarkdownCheckLinksOutputFormat {
    #[default]
    #[serde(alias = "TEXT", alias = "Text")]
    Text,
    #[serde(alias = "JSON", alias = "Json")]
    Json,
    #[serde(alias = "SARIF", alias = "Sarif")]
    Sarif,
}

/// [`super::markdown_links::markdown_check_links_try`] 入参（字段缺省与 runner 内 `parse_markdown_check_args` 一致）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownCheckLinksArgs {
    /// 要扫描的相对路径（`.md` 文件或递归目录）；默认 `README.md` + `docs`
    pub roots: Option<Vec<String>>,
    /// 最多处理多少个 Markdown 文件，默认 300，上限 3000
    #[serde(default)]
    #[schemars(range(min = 1, max = 3000))]
    pub max_files: Option<u32>,
    /// 目录递归深度上限，默认 24，上限 80
    #[serde(default)]
    #[schemars(range(min = 1, max = 80))]
    pub max_depth: Option<u32>,
    /// 仅对这些前缀匹配的 http(s) 或 `//` 外链发起 HEAD；为空则外链仅计数、不联网
    pub allowed_external_prefixes: Option<Vec<String>>,
    /// 外链探测超时（秒），默认 10，上限 60
    #[serde(default)]
    #[schemars(range(min = 1, max = 60))]
    pub external_timeout_secs: Option<u32>,
    /// 是否校验 Markdown 锚点（`#fragment`），默认 true
    #[serde(default = "default_true")]
    pub check_fragments: bool,
    /// 输出格式：text（默认）/ json / sarif
    #[serde(default)]
    pub output_format: Option<MarkdownCheckLinksOutputFormat>,
}

/// [`super::code_metrics::code_stats_try`] 的 `format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeStatsFormat {
    #[default]
    Table,
    Json,
}

/// [`super::code_metrics::code_stats_try`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CodeStatsArgs {
    /// 统计的子路径（相对工作区，默认 `.`）
    pub path: Option<String>,
    #[serde(default)]
    pub format: Option<CodeStatsFormat>,
}

/// [`super::code_metrics::dependency_graph_try`] 的 `format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DependencyGraphFormat {
    #[default]
    Mermaid,
    Dot,
    Tree,
}

/// [`super::code_metrics::dependency_graph_try`] 的 `kind`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DependencyGraphKind {
    #[default]
    Auto,
    Rust,
    Cargo,
    Go,
    Npm,
    Node,
}

/// [`super::code_metrics::dependency_graph_try`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DependencyGraphArgs {
    #[serde(default)]
    pub format: Option<DependencyGraphFormat>,
    /// 依赖树深度（仅 Cargo），默认 1，上限 10
    #[serde(default)]
    #[schemars(range(min = 0, max = 10))]
    pub depth: Option<u32>,
    #[serde(default)]
    pub kind: Option<DependencyGraphKind>,
}

/// [`super::code_metrics::coverage_report_try`] 的 `format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoverageReportFormat {
    #[default]
    Auto,
    Lcov,
    Tarpaulin,
    TarpaulinJson,
    Cobertura,
}

/// [`super::code_metrics::coverage_report_try`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CoverageReportArgs {
    /// 覆盖率报告文件路径（相对工作区）；省略则自动检测
    pub path: Option<String>,
    #[serde(default)]
    pub format: Option<CoverageReportFormat>,
}
