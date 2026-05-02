//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::http_fetch::{HttpFetchArgs, HttpRequestArgs};
use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    CalcArgs, ConvertUnitsArgs, DateCalcArgs, EnvVarCheckArgs, GetCurrentTimeArgs, GetWeatherArgs,
    JsonFormatArgs, RegexTestArgs, TextTransformArgs, WebSearchArgs,
};

pub(in crate::tools) fn params_get_current_time() -> serde_json::Value {
    tool_parameters_schema_value::<GetCurrentTimeArgs>()
}

pub(in crate::tools) fn params_calc() -> serde_json::Value {
    tool_parameters_schema_value::<CalcArgs>()
}

pub(in crate::tools) fn params_convert_units() -> serde_json::Value {
    tool_parameters_schema_value::<ConvertUnitsArgs>()
}

pub(in crate::tools) fn params_weather() -> serde_json::Value {
    tool_parameters_schema_value::<GetWeatherArgs>()
}

pub(in crate::tools) fn params_web_search() -> serde_json::Value {
    tool_parameters_schema_value::<WebSearchArgs>()
}

pub(in crate::tools) fn params_http_fetch() -> serde_json::Value {
    tool_parameters_schema_value::<HttpFetchArgs>()
}

pub(in crate::tools) fn params_http_request() -> serde_json::Value {
    tool_parameters_schema_value::<HttpRequestArgs>()
}

pub(in crate::tools) fn params_text_transform() -> serde_json::Value {
    tool_parameters_schema_value::<TextTransformArgs>()
}

pub(in crate::tools) fn params_regex_test() -> serde_json::Value {
    tool_parameters_schema_value::<RegexTestArgs>()
}

pub(in crate::tools) fn params_date_calc() -> serde_json::Value {
    tool_parameters_schema_value::<DateCalcArgs>()
}

pub(in crate::tools) fn params_json_format() -> serde_json::Value {
    tool_parameters_schema_value::<JsonFormatArgs>()
}

pub(in crate::tools) fn params_env_var_check() -> serde_json::Value {
    tool_parameters_schema_value::<EnvVarCheckArgs>()
}
