//! `docs_health_sweep` 聚合工具的 JSON Schema。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::DocsHealthSweepArgs;

pub(in crate::cm_tools::tools) fn params_docs_health_sweep() -> serde_json::Value {
    tool_parameters_schema_value::<DocsHealthSweepArgs>()
}
