//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{NpmInstallArgs, NpmRunArgs, NpxRunArgs, TscCheckArgs};

pub(in crate::cm_tools::tools) fn params_npm_install() -> serde_json::Value {
    tool_parameters_schema_value::<NpmInstallArgs>()
}

pub(in crate::cm_tools::tools) fn params_npm_run() -> serde_json::Value {
    tool_parameters_schema_value::<NpmRunArgs>()
}

pub(in crate::cm_tools::tools) fn params_npx_run() -> serde_json::Value {
    tool_parameters_schema_value::<NpxRunArgs>()
}

pub(in crate::cm_tools::tools) fn params_tsc_check() -> serde_json::Value {
    tool_parameters_schema_value::<TscCheckArgs>()
}

// ── Go 补充：golangci-lint ──────────────────────────────────
