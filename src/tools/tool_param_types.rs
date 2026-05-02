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
