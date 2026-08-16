//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{FormatOnePathArgs, QualityWorkspaceArgs, RunLintsArgs};

pub(in crate::cm_tools::tools) fn params_format_check_file() -> serde_json::Value {
    tool_parameters_schema_value::<FormatOnePathArgs>()
}

pub(in crate::cm_tools::tools) fn params_quality_workspace() -> serde_json::Value {
    tool_parameters_schema_value::<QualityWorkspaceArgs>()
}

pub(in crate::cm_tools::tools) fn params_format_file() -> serde_json::Value {
    tool_parameters_schema_value::<FormatOnePathArgs>()
}

pub(in crate::cm_tools::tools) fn params_run_lints() -> serde_json::Value {
    tool_parameters_schema_value::<RunLintsArgs>()
}
