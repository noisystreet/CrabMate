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
    pub(crate) fn to_time_output(self) -> crate::tools::time::TimeOutputMode {
        match self {
            Self::Time => crate::tools::time::TimeOutputMode::Time,
            Self::Calendar => crate::tools::time::TimeOutputMode::Calendar,
            Self::Both => crate::tools::time::TimeOutputMode::Both,
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

