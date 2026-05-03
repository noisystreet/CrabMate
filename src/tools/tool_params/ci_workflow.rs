//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    BacktraceAnalyzeArgs, CiPipelineLocalArgs, ReleaseReadyCheckArgs, WorkflowExecuteArgs,
};

pub(in crate::tools) fn params_backtrace_analyze() -> serde_json::Value {
    tool_parameters_schema_value::<BacktraceAnalyzeArgs>()
}

pub(in crate::tools) fn params_ci_pipeline_local() -> serde_json::Value {
    tool_parameters_schema_value::<CiPipelineLocalArgs>()
}

pub(in crate::tools) fn params_release_ready_check() -> serde_json::Value {
    tool_parameters_schema_value::<ReleaseReadyCheckArgs>()
}

pub(in crate::tools) fn params_workflow_execute() -> serde_json::Value {
    tool_parameters_schema_value::<WorkflowExecuteArgs>()
}
