//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{
    BanditScanArgs, CppcheckAnalyzeArgs, HadolintCheckArgs, LizardComplexityArgs, SemgrepScanArgs,
    ShellcheckCheckArgs,
};

pub(in crate::cm_tools::tools) fn params_shellcheck_check() -> serde_json::Value {
    tool_parameters_schema_value::<ShellcheckCheckArgs>()
}

pub(in crate::cm_tools::tools) fn params_cppcheck_analyze() -> serde_json::Value {
    tool_parameters_schema_value::<CppcheckAnalyzeArgs>()
}

pub(in crate::cm_tools::tools) fn params_semgrep_scan() -> serde_json::Value {
    tool_parameters_schema_value::<SemgrepScanArgs>()
}

pub(in crate::cm_tools::tools) fn params_hadolint_check() -> serde_json::Value {
    tool_parameters_schema_value::<HadolintCheckArgs>()
}

pub(in crate::cm_tools::tools) fn params_bandit_scan() -> serde_json::Value {
    tool_parameters_schema_value::<BanditScanArgs>()
}

pub(in crate::cm_tools::tools) fn params_lizard_complexity() -> serde_json::Value {
    tool_parameters_schema_value::<LizardComplexityArgs>()
}
