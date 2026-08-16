//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{
    EmptyToolArgs, GitBlameArgs, GitBranchListArgs, GitDiffArgs, GitDiffBaseArgs, GitDiffNamesArgs,
    GitDiffStatArgs, GitFileHistoryArgs, GitLogArgs, GitShowArgs, GitStatusArgs,
};

pub(in crate::cm_tools::tools) fn params_git_status() -> serde_json::Value {
    tool_parameters_schema_value::<GitStatusArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_clean_check() -> serde_json::Value {
    tool_parameters_schema_value::<EmptyToolArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_diff() -> serde_json::Value {
    tool_parameters_schema_value::<GitDiffArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_diff_stat() -> serde_json::Value {
    tool_parameters_schema_value::<GitDiffStatArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_diff_names() -> serde_json::Value {
    tool_parameters_schema_value::<GitDiffNamesArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_log() -> serde_json::Value {
    tool_parameters_schema_value::<GitLogArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_show() -> serde_json::Value {
    tool_parameters_schema_value::<GitShowArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_diff_base() -> serde_json::Value {
    tool_parameters_schema_value::<GitDiffBaseArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_blame() -> serde_json::Value {
    tool_parameters_schema_value::<GitBlameArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_file_history() -> serde_json::Value {
    tool_parameters_schema_value::<GitFileHistoryArgs>()
}

pub(in crate::cm_tools::tools) fn params_git_branch_list() -> serde_json::Value {
    tool_parameters_schema_value::<GitBranchListArgs>()
}

pub(in crate::cm_tools::tools) fn params_empty_object() -> serde_json::Value {
    tool_parameters_schema_value::<EmptyToolArgs>()
}
