//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::TodoScanArgs;

pub(in crate::tools) fn params_todo_scan() -> serde_json::Value {
    tool_parameters_schema_value::<TodoScanArgs>()
}

// ── 源码分析工具参数 ──────────────────────────────────────────
