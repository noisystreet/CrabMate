//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    GoBuildArgs, GoFmtCheckArgs, GoModTidyArgs, GoTestArgs, GoVetArgs,
};

pub(in crate::tools) fn params_go_build() -> serde_json::Value {
    tool_parameters_schema_value::<GoBuildArgs>()
}

pub(in crate::tools) fn params_go_test() -> serde_json::Value {
    tool_parameters_schema_value::<GoTestArgs>()
}

pub(in crate::tools) fn params_go_vet() -> serde_json::Value {
    tool_parameters_schema_value::<GoVetArgs>()
}

pub(in crate::tools) fn params_go_mod_tidy() -> serde_json::Value {
    tool_parameters_schema_value::<GoModTidyArgs>()
}

pub(in crate::tools) fn params_go_fmt_check() -> serde_json::Value {
    tool_parameters_schema_value::<GoFmtCheckArgs>()
}

// ── Go 补充：golangci-lint ──────────────────────────────────
