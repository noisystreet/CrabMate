//! 内置工具入参的 **serde + schemars** 真源：与 [`super::tool_json_schema`] 生成的 `parameters` 及
//! 各 `runner_*` 反序列化形状一致；逐步把仍用手写 `json!` 的工具迁到本模块。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// [`super::calc`] 工具入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalcArgs {
    /// 数学表达式，如 1+2*3、2^10、sqrt(2)、s(pi/2)、math::log10(100)
    pub expression: String,
}

/// [`super::time`] 工具 `mode` 取值（与历史字符串一致，小写）。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GetCurrentTimeMode {
    Time,
    Calendar,
    Both,
}

/// [`super::time::run`] 对应工具入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GetCurrentTimeArgs {
    /// 输出模式：time / calendar / both；默认 time。
    pub mode: Option<GetCurrentTimeMode>,
    /// 可选：日历年份（仅在 calendar/both 时生效）
    pub year: Option<i32>,
    /// 可选：日历月份 1–12（仅在 calendar/both 时生效）
    pub month: Option<u32>,
}

impl GetCurrentTimeMode {
    pub(crate) fn to_time_output(self) -> super::time::TimeOutputMode {
        match self {
            Self::Time => super::time::TimeOutputMode::Time,
            Self::Calendar => super::time::TimeOutputMode::Calendar,
            Self::Both => super::time::TimeOutputMode::Both,
        }
    }
}

/// [`super::unit_convert::run`] 入参；`category` 运行时仍按 `unit_convert` 规则解析（含中文别名）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertUnitsArgs {
    pub category: String,
    pub value: f64,
    pub from: String,
    pub to: String,
}

/// [`super::weather::run`] 入参（`city` 与 `location` 二选一，至少 2 字符由 runner 校验）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GetWeatherArgs {
    pub city: Option<String>,
    pub location: Option<String>,
}

/// [`super::web_search::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebSearchArgs {
    pub query: String,
    /// 1～20；省略时用配置默认
    pub max_results: Option<u64>,
}

/// [`super::regex_test::run`] 入参。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextTransformOp {
    Base64Encode,
    Base64Decode,
    UrlEncode,
    UrlDecode,
    HashShort,
    LinesJoin,
    LinesSplit,
}

/// `hash_short` 所用算法。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextTransformHashAlgo {
    #[default]
    Sha256,
    Blake3,
}

/// [`super::text_transform::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextTransformArgs {
    pub op: TextTransformOp,
    pub text: String,
    pub delimiter: Option<String>,
    #[serde(default)]
    pub hash_algo: Option<TextTransformHashAlgo>,
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

/// [`super::process_tools::port_check`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortCheckArgs {
    /// 要检查的端口号（1–65535）
    #[schemars(range(min = 1, max = 65535))]
    pub port: u32,
}

/// [`super::process_tools::process_list`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessListArgs {
    /// 按进程名/命令行关键词过滤（不区分大小写）
    pub filter: Option<String>,
    /// 是否仅当前用户进程，默认 true
    #[serde(default = "default_true")]
    pub user_only: bool,
    /// 最多返回条数，默认 100，上限 500
    #[serde(default = "default_process_list_max_count")]
    #[schemars(range(min = 1, max = 500))]
    pub max_count: u32,
}

fn default_true() -> bool {
    true
}

fn default_process_list_max_count() -> u32 {
    100
}

/// [`super::go_tools::golangci_lint`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GolangciLintArgs {
    /// 是否 `--fix` 自动修复，默认 false
    #[serde(default)]
    pub fix: bool,
    /// 是否 `--fast` 快速模式，默认 false
    #[serde(default)]
    pub fast: bool,
}

/// [`super::markdown_links::markdown_check_links`] 的 `output_format`。
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

/// [`super::markdown_links::markdown_check_links`] 入参（字段缺省与 runner 内 `parse_markdown_check_args` 一致）。
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

/// [`super::todo_scan::run`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TodoScanArgs {
    /// 扫描路径（相对工作区，默认 `[\".\"]`）
    pub paths: Option<Vec<String>>,
    /// 标记列表（默认 TODO / FIXME / HACK / XXX）
    pub markers: Option<Vec<String>>,
    /// 排除目录名（默认 target、node_modules 等）
    pub exclude: Option<Vec<String>>,
}

/// [`super::code_metrics::code_stats`] 的 `format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeStatsFormat {
    #[default]
    Table,
    Json,
}

/// [`super::code_metrics::code_stats`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CodeStatsArgs {
    /// 统计的子路径（相对工作区，默认 `.`）
    pub path: Option<String>,
    #[serde(default)]
    pub format: Option<CodeStatsFormat>,
}

/// [`super::code_metrics::dependency_graph`] 的 `format`。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DependencyGraphFormat {
    #[default]
    Mermaid,
    Dot,
    Tree,
}

/// [`super::code_metrics::dependency_graph`] 的 `kind`。
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

/// [`super::code_metrics::dependency_graph`] 入参。
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

/// [`super::code_metrics::coverage_report`] 的 `format`。
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

/// [`super::code_metrics::coverage_report`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CoverageReportArgs {
    /// 覆盖率报告文件路径（相对工作区）；省略则自动检测
    pub path: Option<String>,
    #[serde(default)]
    pub format: Option<CoverageReportFormat>,
}

/// [`super::package_query::run`] 的包管理器偏好。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PackageQueryManagerPref {
    #[default]
    Auto,
    Apt,
    Rpm,
}

/// [`super::package_query::run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageQueryArgs {
    pub package: String,
    #[serde(default, deserialize_with = "deserialize_package_query_manager")]
    pub manager: PackageQueryManagerPref,
}

fn deserialize_package_query_manager<'de, D>(
    deserializer: D,
) -> Result<PackageQueryManagerPref, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let opt = Option::<String>::deserialize(deserializer)?;
    let raw = opt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    match raw.to_ascii_lowercase().as_str() {
        "auto" => Ok(PackageQueryManagerPref::Auto),
        "apt" => Ok(PackageQueryManagerPref::Apt),
        "rpm" => Ok(PackageQueryManagerPref::Rpm),
        _ => Err(Error::custom(
            "manager 仅支持 auto / apt / rpm（大小写不敏感；可省略）",
        )),
    }
}

// ── Node.js / npm（`nodejs_tools`）────────────────────────────

/// [`super::nodejs_tools::npm_install`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct NpmInstallArgs {
    pub subdir: Option<String>,
    #[serde(default)]
    pub ci: bool,
    #[serde(default)]
    pub production: bool,
}

/// [`super::nodejs_tools::npm_run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NpmRunArgs {
    pub script: String,
    pub subdir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// [`super::nodejs_tools::npx_run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NpxRunArgs {
    pub package: String,
    pub subdir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// [`super::nodejs_tools::tsc_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TscCheckArgs {
    pub subdir: Option<String>,
    pub project: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

// ── Go（`go_tools`，不含 `golangci_lint`）──────────────────────

/// [`super::go_tools::go_build`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoBuildArgs {
    pub package: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub race: bool,
    #[serde(default)]
    pub verbose: bool,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoTestArgs {
    pub package: Option<String>,
    pub run: Option<String>,
    #[serde(default)]
    pub race: bool,
    #[serde(default = "default_true")]
    pub verbose: bool,
    #[serde(default)]
    pub short: bool,
    pub count: Option<u64>,
    pub timeout: Option<String>,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_vet`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoVetArgs {
    pub package: Option<String>,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_mod_tidy`] 入参（runner 仅消费 `confirm`；`verbose` 与历史 schema 对齐）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoModTidyArgs {
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub confirm: bool,
}

/// [`super::go_tools::go_fmt_check`] 入参（与 runner 一致：单键 `path`，默认 `.`）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoFmtCheckArgs {
    pub path: Option<String>,
}

// ── 容器（`container_tools`）──────────────────────────────────

/// [`super::container_tools::docker_build`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DockerBuildArgs {
    pub context: Option<String>,
    pub tag: Option<String>,
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub no_cache: bool,
}

/// [`super::container_tools::docker_compose_ps`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DockerComposePsArgs {
    pub project: Option<String>,
    #[serde(default)]
    pub compose_files: Vec<String>,
}

/// [`super::container_tools::podman_images`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct PodmanImagesArgs {
    pub reference: Option<String>,
}

// ── 单文件 path（`format`）────────────────────────────────────

/// [`super::format::run`] / [`super::format::run_check`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormatOnePathArgs {
    pub path: String,
}

// ── `lint::run` ───────────────────────────────────────────────

/// [`super::lint::run`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct RunLintsArgs {
    #[serde(default = "default_true")]
    pub run_cargo: bool,
    #[serde(default = "default_true")]
    pub run_cargo_check: bool,
    #[serde(default = "default_true")]
    pub run_frontend: bool,
    #[serde(default)]
    pub run_frontend_build: bool,
    #[serde(default = "default_true")]
    pub run_python_ruff: bool,
}

// ── `quality_tools::quality_workspace` ────────────────────────

/// [`super::quality_tools::quality_workspace`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct QualityWorkspaceArgs {
    #[serde(default = "default_true")]
    pub run_cargo_fmt_check: bool,
    #[serde(default)]
    pub run_cargo_check: bool,
    #[serde(default = "default_true")]
    pub run_cargo_clippy: bool,
    #[serde(default)]
    pub run_cargo_test: bool,
    #[serde(default)]
    pub run_frontend_lint: bool,
    #[serde(default)]
    pub run_frontend_build: bool,
    #[serde(default)]
    pub run_frontend_prettier_check: bool,
    #[serde(default)]
    pub run_ruff_check: bool,
    #[serde(default)]
    pub run_pytest: bool,
    #[serde(default)]
    pub run_mypy: bool,
    #[serde(default)]
    pub run_maven_compile: bool,
    #[serde(default)]
    pub run_maven_test: bool,
    #[serde(default)]
    pub run_gradle_compile: bool,
    #[serde(default)]
    pub run_gradle_test: bool,
    #[serde(default)]
    pub run_docker_compose_ps: bool,
    #[serde(default)]
    pub run_podman_images: bool,
    #[serde(default = "default_true")]
    pub fail_fast: bool,
    #[serde(default)]
    pub summary_only: bool,
}

// ── 源码分析（`source_analysis_tools`）────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckSeverity {
    Error,
    Warning,
    Info,
    Style,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckShellDialect {
    Sh,
    Bash,
    Dash,
    Ksh,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckOutputFormat {
    Tty,
    Gcc,
    #[serde(rename = "json1")]
    Json1,
    Checkstyle,
    Diff,
    Quiet,
}

/// [`super::source_analysis_tools::shellcheck_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ShellcheckCheckArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub severity: Option<ShellcheckSeverity>,
    pub shell: Option<ShellcheckShellDialect>,
    pub format: Option<ShellcheckOutputFormat>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CppcheckPlatform {
    #[serde(rename = "unix32")]
    Unix32,
    #[serde(rename = "unix64")]
    Unix64,
    #[serde(rename = "win32A")]
    Win32a,
    #[serde(rename = "win32W")]
    Win32w,
    #[serde(rename = "win64")]
    Win64,
    Native,
}

/// [`super::source_analysis_tools::cppcheck_analyze`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CppcheckAnalyzeArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub enable: Option<String>,
    pub std: Option<String>,
    pub platform: Option<CppcheckPlatform>,
}

/// [`super::source_analysis_tools::semgrep_scan`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct SemgrepScanArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub config: Option<String>,
    pub severity: Option<String>,
    pub lang: Option<String>,
    #[serde(default)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HadolintOutputFormat {
    Tty,
    Json,
    Checkstyle,
    Codeclimate,
    #[serde(rename = "gitlab_codeclimate")]
    GitlabCodeclimate,
    Gnu,
    Codacy,
    Sonarqube,
    Sarif,
}

/// [`super::source_analysis_tools::hadolint_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct HadolintCheckArgs {
    pub path: Option<String>,
    pub format: Option<HadolintOutputFormat>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub trusted_registries: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditSeverityArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditConfidenceArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditOutputFormat {
    Txt,
    Json,
    Csv,
    Xml,
    Html,
    Yaml,
    Screen,
    Custom,
}

/// [`super::source_analysis_tools::bandit_scan`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct BanditScanArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub severity: Option<BanditSeverityArg>,
    pub confidence: Option<BanditConfidenceArg>,
    pub skip: Option<String>,
    pub format: Option<BanditOutputFormat>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LizardSortKind {
    CyclomaticComplexity,
    Length,
    TokenCount,
    ParameterCount,
    Nloc,
}

/// [`super::source_analysis_tools::lizard_complexity`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct LizardComplexityArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 200))]
    pub threshold: Option<u32>,
    pub language: Option<String>,
    pub sort: Option<LizardSortKind>,
    #[serde(default)]
    pub warnings_only: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
}

// ── 日程与提醒（`schedule`）────────────────────────────────────

/// [`super::schedule::add_reminder`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddReminderArgs {
    pub title: String,
    pub due_at: Option<String>,
}

/// [`super::schedule::list_reminders`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListRemindersArgs {
    #[serde(default)]
    pub include_done: bool,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
}

/// [`super::schedule::update_reminder`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateReminderArgs {
    pub id: String,
    pub title: Option<String>,
    pub due_at: Option<String>,
    pub done: Option<bool>,
}

/// 仅含 `id` 的日程/提醒工具入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdOnlyArgs {
    pub id: String,
}

/// [`super::schedule::add_event`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddEventArgs {
    pub title: String,
    pub start_at: String,
    pub end_at: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

/// [`super::schedule::list_events`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListEventsArgs {
    pub year: Option<i32>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 12))]
    pub month: Option<u32>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
}

/// [`super::schedule::update_event`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateEventArgs {
    pub id: String,
    pub title: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

// ── JVM（`jvm_tools`）──────────────────────────────────────────

/// [`super::jvm_tools::maven_compile`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct MavenCompileArgs {
    pub profile: Option<String>,
}

/// [`super::jvm_tools::maven_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct MavenTestArgs {
    pub profile: Option<String>,
    pub test: Option<String>,
}

/// [`super::jvm_tools::gradle_compile`] / [`super::jvm_tools::gradle_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GradleTasksArgs {
    #[serde(default)]
    pub tasks: Vec<String>,
}

// ── 归档（`archive`）──────────────────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum ArchivePackFormat {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tar.xz")]
    TarXz,
}

/// [`super::archive::archive_pack`] 入参（`exclude` / `format` 与 schema 对齐，当前实现仍按输出扩展名推断格式）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchivePackArgs {
    pub output: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub format: Option<ArchivePackFormat>,
}

/// [`super::archive::archive_unpack`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveUnpackArgs {
    pub archive: String,
    #[serde(default = "default_dot_str")]
    pub output_dir: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub strip_components: Option<u32>,
}

fn default_dot_str() -> String {
    ".".to_string()
}

/// [`super::archive::archive_list`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveListArgs {
    pub archive: String,
    #[serde(default)]
    pub verbose: bool,
}

// ── 代码导航（`symbol` / `code_nav` / `call_graph_sketch`）──────

/// [`super::symbol::run`]（`find_symbol`）入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindSymbolArgs {
    pub symbol: String,
    pub path: Option<String>,
    pub kind: Option<String>,
    #[serde(default = "default_find_symbol_max_results")]
    #[schemars(range(min = 1, max = 200))]
    pub max_results: Option<u64>,
    #[serde(default = "default_context_lines")]
    #[schemars(range(min = 0))]
    pub context_lines: Option<u64>,
    #[serde(default = "default_true")]
    pub case_insensitive: bool,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_find_symbol_max_results() -> Option<u64> {
    Some(30)
}

fn default_context_lines() -> Option<u64> {
    Some(2)
}

/// [`super::code_nav::find_references`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindReferencesArgs {
    pub symbol: String,
    pub path: Option<String>,
    #[serde(default = "default_find_refs_max_results")]
    #[schemars(range(min = 1, max = 300))]
    pub max_results: Option<u64>,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default = "default_true")]
    pub exclude_definitions: bool,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_find_refs_max_results() -> Option<u64> {
    Some(80)
}

/// [`super::call_graph_sketch::run`] 入参（`symbol` 与 `symbols` 至少其一由 runner 校验）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CallGraphSketchArgs {
    #[serde(default)]
    pub symbols: Vec<String>,
    pub symbol: Option<String>,
    pub path: Option<String>,
    #[serde(default = "default_call_graph_max_edges")]
    #[schemars(range(min = 1, max = 3000))]
    pub max_edges: Option<u64>,
    #[serde(default = "default_call_graph_max_files")]
    #[schemars(range(min = 1, max = 50000))]
    pub max_files: Option<u64>,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_call_graph_max_edges() -> Option<u64> {
    Some(400)
}

fn default_call_graph_max_files() -> Option<u64> {
    Some(12_000)
}
