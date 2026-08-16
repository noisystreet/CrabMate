//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{CodeStatsArgs, CoverageReportArgs, DependencyGraphArgs};

pub(in crate::cm_tools::tools) fn params_code_stats() -> serde_json::Value {
    tool_parameters_schema_value::<CodeStatsArgs>()
}

pub(in crate::cm_tools::tools) fn params_dependency_graph() -> serde_json::Value {
    tool_parameters_schema_value::<DependencyGraphArgs>()
}

pub(in crate::cm_tools::tools) fn params_coverage_report() -> serde_json::Value {
    tool_parameters_schema_value::<CoverageReportArgs>()
}

// ── 文件工具增强 ────────────────────────────────────────────
