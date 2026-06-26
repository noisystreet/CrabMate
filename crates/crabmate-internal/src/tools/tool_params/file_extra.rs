//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    AppendFileArgs, ChmodFileArgs, CreateDirArgs, DeleteDirArgs, DeleteFileArgs, SearchReplaceArgs,
    SymlinkInfoArgs,
};

pub(in crate::tools) fn params_delete_file() -> serde_json::Value {
    tool_parameters_schema_value::<DeleteFileArgs>()
}

pub(in crate::tools) fn params_delete_dir() -> serde_json::Value {
    tool_parameters_schema_value::<DeleteDirArgs>()
}

pub(in crate::tools) fn params_append_file() -> serde_json::Value {
    tool_parameters_schema_value::<AppendFileArgs>()
}

pub(in crate::tools) fn params_create_dir() -> serde_json::Value {
    tool_parameters_schema_value::<CreateDirArgs>()
}

pub(in crate::tools) fn params_search_replace() -> serde_json::Value {
    tool_parameters_schema_value::<SearchReplaceArgs>()
}

pub(in crate::tools) fn params_chmod_file() -> serde_json::Value {
    tool_parameters_schema_value::<ChmodFileArgs>()
}

pub(in crate::tools) fn params_symlink_info() -> serde_json::Value {
    tool_parameters_schema_value::<SymlinkInfoArgs>()
}
