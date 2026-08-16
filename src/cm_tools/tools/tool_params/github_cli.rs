//! GitHub CLI（`gh`）封装工具的 JSON Schema。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{
    GhApiArgs, GhIssueCreateArgs, GhIssueListArgs, GhIssueViewArgs, GhPrBodyDraftArgs,
    GhPrChecksArgs, GhPrCommentArgs, GhPrCreateArgs, GhPrDiffArgs, GhPrEditArgs, GhPrListArgs,
    GhPrMergeArgs, GhPrReviewArgs, GhPrViewArgs, GhReleaseCreateArgs, GhReleaseListArgs,
    GhReleaseViewArgs, GhRunFailureSummaryArgs, GhRunListArgs, GhRunRerunArgs, GhRunViewArgs,
    GhSearchArgs,
};

pub(in crate::cm_tools::tools) fn params_gh_pr_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrListArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrViewArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_checks() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrChecksArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_create() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrCreateArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_merge() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrMergeArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_review() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrReviewArgs>()
}
pub(in crate::cm_tools::tools) fn params_gh_pr_comment() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrCommentArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_edit() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrEditArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_body_draft() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrBodyDraftArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_issue_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhIssueListArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_issue_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhIssueViewArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_issue_create() -> serde_json::Value {
    tool_parameters_schema_value::<GhIssueCreateArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_run_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunListArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_pr_diff() -> serde_json::Value {
    tool_parameters_schema_value::<GhPrDiffArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_run_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunViewArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_run_rerun() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunRerunArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_run_failure_summary() -> serde_json::Value {
    tool_parameters_schema_value::<GhRunFailureSummaryArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_release_list() -> serde_json::Value {
    tool_parameters_schema_value::<GhReleaseListArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_release_view() -> serde_json::Value {
    tool_parameters_schema_value::<GhReleaseViewArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_release_create() -> serde_json::Value {
    tool_parameters_schema_value::<GhReleaseCreateArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_search() -> serde_json::Value {
    tool_parameters_schema_value::<GhSearchArgs>()
}

pub(in crate::cm_tools::tools) fn params_gh_api() -> serde_json::Value {
    tool_parameters_schema_value::<GhApiArgs>()
}
