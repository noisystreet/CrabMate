//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{CallGraphSketchArgs, FindReferencesArgs, FindSymbolArgs};

pub(in crate::tools) fn params_find_symbol() -> serde_json::Value {
    tool_parameters_schema_value::<FindSymbolArgs>()
}

pub(in crate::tools) fn params_find_references() -> serde_json::Value {
    tool_parameters_schema_value::<FindReferencesArgs>()
}

pub(in crate::tools) fn params_call_graph_sketch() -> serde_json::Value {
    tool_parameters_schema_value::<CallGraphSketchArgs>()
}
