//! 内置工具入参的 **serde + schemars** 真源：与 [`super::tool_json_schema`] 生成的 `parameters` 及
//! 各 `runner_*` 反序列化形状一致；逐步把仍用手写 `json!` 的工具迁到本模块。

use schemars::JsonSchema;
use serde::Deserialize;

/// [`super::calc`] 工具入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalcArgs {
    /// 数学表达式，如 1+2*3、2^10、sqrt(2)、s(pi/2)、math::log10(100)
    pub expression: String,
}

/// [`super::time`] 工具 `mode` 取值（与历史字符串一致，小写）。
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GetCurrentTimeMode {
    Time,
    Calendar,
    Both,
}

/// [`super::time::run`] 对应工具入参。
#[derive(Debug, Default, Deserialize, JsonSchema)]
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
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertUnitsArgs {
    pub category: String,
    pub value: f64,
    pub from: String,
    pub to: String,
}

/// [`super::weather::run`] 入参（`city` 与 `location` 二选一，至少 2 字符由 runner 校验）。
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GetWeatherArgs {
    pub city: Option<String>,
    pub location: Option<String>,
}

/// [`super::web_search::run`] 入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebSearchArgs {
    pub query: String,
    /// 1～20；省略时用配置默认
    pub max_results: Option<u64>,
}

/// [`super::regex_test::run`] 入参。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextTransformHashAlgo {
    #[default]
    Sha256,
    Blake3,
}

/// [`super::text_transform::run`] 入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextTransformArgs {
    pub op: TextTransformOp,
    pub text: String,
    pub delimiter: Option<String>,
    #[serde(default)]
    pub hash_algo: Option<TextTransformHashAlgo>,
}

/// [`super::regex_test::run`] 入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegexTestArgs {
    pub pattern: String,
    pub test_strings: Vec<String>,
}

/// [`super::date_calc::run`] 的 `mode`。
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DateCalcMode {
    #[default]
    Offset,
    Diff,
}

/// [`super::date_calc::run`] 入参。
#[derive(Debug, Default, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JsonFormatMode {
    Pretty,
    Compact,
    YamlToJson,
    JsonToYaml,
}

/// [`super::json_format::run`] 入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonFormatArgs {
    pub text: String,
    #[serde(default)]
    pub mode: Option<JsonFormatMode>,
}

/// [`super::env_var_check::run`] 入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvVarCheckArgs {
    pub names: Vec<String>,
    #[serde(default)]
    pub show_length: Option<bool>,
    pub show_prefix_chars: Option<u64>,
}
