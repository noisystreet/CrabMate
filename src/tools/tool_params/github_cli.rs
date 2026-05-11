//! GitHub CLI（`gh`）封装工具的 JSON Schema。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    GhApiArgs, GhIssueListArgs, GhIssueViewArgs, GhPrChecksArgs, GhPrCreateArgs, GhPrDiffArgs,
    GhPrListArgs, GhPrViewArgs, GhReleaseListArgs, GhReleaseViewArgs, GhRunListArgs, GhRunViewArgs,
    GhSearchArgs,
};

pub(in crate::tools) fn params_gh_pr_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrListArgs>()
}

pub(in crate::tools) fn params_gh_pr_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrViewArgs>()
}

pub(in crate::tools) fn params_gh_pr_checks() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrChecksArgs>()
}

pub(in crate::tools) fn params_gh_pr_create() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrCreateArgs>()
}

pub(in crate::tools) fn params_gh_issue_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhIssueListArgs>()
}

pub(in crate::tools) fn params_gh_issue_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhIssueViewArgs>()
}

pub(in crate::tools) fn params_gh_run_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunListArgs>()
}

pub(in crate::tools) fn params_gh_pr_diff() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrDiffArgs>()
}

pub(in crate::tools) fn params_gh_run_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunViewArgs>()
}

pub(in crate::tools) fn params_gh_release_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhReleaseListArgs>()
}

pub(in crate::tools) fn params_gh_release_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhReleaseViewArgs>()
}

pub(in crate::tools) fn params_gh_search() -> serde_json::Value {
    tool_parameters_schema_value::<GhSearchArgs>()
}

pub(in crate::tools) fn params_gh_api() -> serde_json::Value {
    tool_parameters_schema_value::<GhApiArgs>()
}
