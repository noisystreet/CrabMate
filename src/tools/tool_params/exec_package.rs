//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{PackageQueryArgs, RunCommandArgs, TerminalSessionArgs};

pub(in crate::tools) fn params_terminal_session() -> serde_json::Value {
    tool_parameters_schema_value::<TerminalSessionArgs>()
}

pub(in crate::tools) fn params_run_command() -> serde_json::Value {
    tool_parameters_schema_value::<RunCommandArgs>()
}

pub(in crate::tools) fn params_package_query() -> serde_json::Value {
    tool_parameters_schema_value::<PackageQueryArgs>()
}
