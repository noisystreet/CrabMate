//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{PortCheckArgs, ProcessListArgs};

pub(in crate::tools) fn params_port_check() -> serde_json::Value {
    tool_parameters_schema_value::<PortCheckArgs>()
}

pub(in crate::tools) fn params_process_list() -> serde_json::Value {
    tool_parameters_schema_value::<ProcessListArgs>()
}

// ── 代码度量与分析 ──────────────────────────────────────────
