//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    ApplyPatchArgs, CodebaseSemanticSearchArgs, ExtractInFileArgs, FileExistsArgs,
    FileFromToOverwriteArgs, FileWriteArgs, GlobFilesArgs, HashFileArgs, ListTreeArgs,
    ModifyFileArgs, ReadBinaryMetaArgs, ReadDirEnhancedArgs, ReadFileArgs,
    SearchInFilesEnhancedArgs,
};

pub(in crate::tools) fn params_file_write() -> serde_json::Value {
    tool_parameters_schema_value::<FileWriteArgs>()
}

pub(in crate::tools) fn params_modify_file() -> serde_json::Value {
    tool_parameters_schema_value::<ModifyFileArgs>()
}

pub(in crate::tools) fn params_file_from_to_overwrite() -> serde_json::Value {
    tool_parameters_schema_value::<FileFromToOverwriteArgs>()
}

pub(in crate::tools) fn params_read_file() -> serde_json::Value {
    tool_parameters_schema_value::<ReadFileArgs>()
}

pub(in crate::tools) fn params_glob_files() -> serde_json::Value {
    tool_parameters_schema_value::<GlobFilesArgs>()
}

pub(in crate::tools) fn params_list_tree() -> serde_json::Value {
    tool_parameters_schema_value::<ListTreeArgs>()
}

pub(in crate::tools) fn params_file_exists() -> serde_json::Value {
    tool_parameters_schema_value::<FileExistsArgs>()
}

pub(in crate::tools) fn params_read_binary_meta() -> serde_json::Value {
    tool_parameters_schema_value::<ReadBinaryMetaArgs>()
}

pub(in crate::tools) fn params_hash_file() -> serde_json::Value {
    tool_parameters_schema_value::<HashFileArgs>()
}

pub(in crate::tools) fn params_extract_in_file() -> serde_json::Value {
    tool_parameters_schema_value::<ExtractInFileArgs>()
}

pub(in crate::tools) fn params_apply_patch() -> serde_json::Value {
    tool_parameters_schema_value::<ApplyPatchArgs>()
}

pub(in crate::tools) fn params_codebase_semantic_search() -> serde_json::Value {
    tool_parameters_schema_value::<CodebaseSemanticSearchArgs>()
}

pub(in crate::tools) fn params_search_in_files_enhanced() -> serde_json::Value {
    tool_parameters_schema_value::<SearchInFilesEnhancedArgs>()
}

pub(in crate::tools) fn params_read_dir_enhanced() -> serde_json::Value {
    tool_parameters_schema_value::<ReadDirEnhancedArgs>()
}
