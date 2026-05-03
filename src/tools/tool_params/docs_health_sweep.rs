//! `docs_health_sweep` 聚合工具的 JSON Schema。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::DocsHealthSweepArgs;

pub(in crate::tools) fn params_docs_health_sweep() -> serde_json::Value {
    tool_parameters_schema_value::<DocsHealthSweepArgs>()
}
