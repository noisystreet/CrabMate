//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::GolangciLintArgs;

pub(in crate::cm_tools::tools) fn params_golangci_lint() -> serde_json::Value {
    tool_parameters_schema_value::<GolangciLintArgs>()
}

// ── 进程与端口管理 ──────────────────────────────────────────
