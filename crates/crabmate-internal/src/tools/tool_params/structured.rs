//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    StructuredDiffArgs, StructuredPatchArgs, StructuredQueryArgs, StructuredValidateArgs,
};

pub(in crate::tools) fn params_structured_validate() -> serde_json::Value {
    tool_parameters_schema_value::<StructuredValidateArgs>()
}

pub(in crate::tools) fn params_structured_query() -> serde_json::Value {
    tool_parameters_schema_value::<StructuredQueryArgs>()
}

pub(in crate::tools) fn params_structured_diff() -> serde_json::Value {
    tool_parameters_schema_value::<StructuredDiffArgs>()
}

pub(in crate::tools) fn params_structured_patch() -> serde_json::Value {
    tool_parameters_schema_value::<StructuredPatchArgs>()
}
