//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{FormatOnePathArgs, QualityWorkspaceArgs, RunLintsArgs};

pub(in crate::tools) fn params_format_check_file() -> serde_json::Value {
    tool_parameters_schema_value::<FormatOnePathArgs>()
}

pub(in crate::tools) fn params_quality_workspace() -> serde_json::Value {
    tool_parameters_schema_value::<QualityWorkspaceArgs>()
}

pub(in crate::tools) fn params_format_file() -> serde_json::Value {
    tool_parameters_schema_value::<FormatOnePathArgs>()
}

pub(in crate::tools) fn params_run_lints() -> serde_json::Value {
    tool_parameters_schema_value::<RunLintsArgs>()
}
