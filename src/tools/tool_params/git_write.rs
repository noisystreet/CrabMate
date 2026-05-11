//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    GitApplyArgs, GitBranchCreateArgs, GitBranchDeleteArgs, GitCheckoutArgs, GitCherryPickArgs,
    GitCloneArgs, GitCommitArgs, GitFetchArgs, GitMergeArgs, GitPushArgs, GitRebaseArgs,
    GitRemoteSetUrlArgs, GitResetArgs, GitRevertArgs, GitStageFilesArgs, GitStashArgs, GitTagArgs,
};

pub(in crate::tools) fn params_git_stage_files() -> serde_json::Value {
    tool_parameters_schema_value::<GitStageFilesArgs>()
}

pub(in crate::tools) fn params_git_commit() -> serde_json::Value {
    tool_parameters_schema_value::<GitCommitArgs>()
}

pub(in crate::tools) fn params_git_fetch() -> serde_json::Value {
    tool_parameters_schema_value::<GitFetchArgs>()
}

pub(in crate::tools) fn params_git_remote_set_url() -> serde_json::Value {
    tool_parameters_schema_value::<GitRemoteSetUrlArgs>()
}

pub(in crate::tools) fn params_git_apply() -> serde_json::Value {
    tool_parameters_schema_value::<GitApplyArgs>()
}

pub(in crate::tools) fn params_git_clone() -> serde_json::Value {
    tool_parameters_schema_value::<GitCloneArgs>()
}

pub(in crate::tools) fn params_git_checkout() -> serde_json::Value {
    tool_parameters_schema_value::<GitCheckoutArgs>()
}

pub(in crate::tools) fn params_git_branch_create() -> serde_json::Value {
    tool_parameters_schema_value::<GitBranchCreateArgs>()
}

pub(in crate::tools) fn params_git_branch_delete() -> serde_json::Value {
    tool_parameters_schema_value::<GitBranchDeleteArgs>()
}

pub(in crate::tools) fn params_git_push() -> serde_json::Value {
    tool_parameters_schema_value::<GitPushArgs>()
}

pub(in crate::tools) fn params_git_merge() -> serde_json::Value {
    tool_parameters_schema_value::<GitMergeArgs>()
}

pub(in crate::tools) fn params_git_rebase() -> serde_json::Value {
    tool_parameters_schema_value::<GitRebaseArgs>()
}

pub(in crate::tools) fn params_git_stash() -> serde_json::Value {
    tool_parameters_schema_value::<GitStashArgs>()
}

pub(in crate::tools) fn params_git_tag() -> serde_json::Value {
    tool_parameters_schema_value::<GitTagArgs>()
}

pub(in crate::tools) fn params_git_reset() -> serde_json::Value {
    tool_parameters_schema_value::<GitResetArgs>()
}

pub(in crate::tools) fn params_git_cherry_pick() -> serde_json::Value {
    tool_parameters_schema_value::<GitCherryPickArgs>()
}

pub(in crate::tools) fn params_git_revert() -> serde_json::Value {
    tool_parameters_schema_value::<GitRevertArgs>()
}
