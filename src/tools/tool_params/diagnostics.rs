//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    ChangelogDraftArgs, CrateContractMapArgs, DiagnosticSummaryArgs, ErrorOutputPlaybookArgs,
    LicenseNoticeArgs, LongTermForgetArgs, LongTermMemoryListArgs, LongTermRememberArgs,
    PlaybookRunCommandsArgs, PresentClarificationQuestionnaireArgs, RepoOverviewSweepArgs,
    SummarizeExperienceArgs,
};

pub(in crate::tools) fn params_changelog_draft() -> serde_json::Value {
    tool_parameters_schema_value::<ChangelogDraftArgs>()
}

pub(in crate::tools) fn params_license_notice() -> serde_json::Value {
    tool_parameters_schema_value::<LicenseNoticeArgs>()
}

pub(in crate::tools) fn params_present_clarification_questionnaire() -> serde_json::Value {
    tool_parameters_schema_value::<PresentClarificationQuestionnaireArgs>()
}

pub(in crate::tools) fn params_diagnostic_summary() -> serde_json::Value {
    tool_parameters_schema_value::<DiagnosticSummaryArgs>()
}

pub(in crate::tools) fn params_repo_overview_sweep() -> serde_json::Value {
    tool_parameters_schema_value::<RepoOverviewSweepArgs>()
}

pub(in crate::tools) fn params_crate_contract_map() -> serde_json::Value {
    tool_parameters_schema_value::<CrateContractMapArgs>()
}

pub(in crate::tools) fn params_error_output_playbook() -> serde_json::Value {
    tool_parameters_schema_value::<ErrorOutputPlaybookArgs>()
}

pub(in crate::tools) fn params_long_term_remember() -> serde_json::Value {
    tool_parameters_schema_value::<LongTermRememberArgs>()
}

pub(in crate::tools) fn params_long_term_forget() -> serde_json::Value {
    tool_parameters_schema_value::<LongTermForgetArgs>()
}

pub(in crate::tools) fn params_long_term_memory_list() -> serde_json::Value {
    tool_parameters_schema_value::<LongTermMemoryListArgs>()
}

pub(in crate::tools) fn params_summarize_experience() -> serde_json::Value {
    tool_parameters_schema_value::<SummarizeExperienceArgs>()
}

pub(in crate::tools) fn params_playbook_run_commands() -> serde_json::Value {
    tool_parameters_schema_value::<PlaybookRunCommandsArgs>()
}
