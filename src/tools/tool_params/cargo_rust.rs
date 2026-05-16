//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    CargoAuditArgs, CargoCleanArgs, CargoCommonCliArgs, CargoDenyArgs, CargoDocArgs, CargoFixArgs,
    CargoMacheteArgs, CargoMetadataArgs, CargoNextestArgs, CargoOutdatedArgs,
    CargoPublishDryRunArgs, CargoRunArgs, CargoTestArgs, CargoTreeArgs, CargoUdepsArgs,
    EmptyToolArgs, RustAnalyzerDocumentSymbolArgs, RustAnalyzerPositionArgs,
    RustAnalyzerReferencesArgs, RustAnalyzerWorkspaceSymbolArgs, RustCompilerJsonArgs,
    RustFileOutlineArgs, RustRustcArgs, RustTestOneArgs,
};

pub(in crate::tools) fn params_cargo_common() -> serde_json::Value {
    tool_parameters_schema_value::<CargoCommonCliArgs>()
}

pub(in crate::tools) fn params_cargo_test() -> serde_json::Value {
    tool_parameters_schema_value::<CargoTestArgs>()
}

pub(in crate::tools) fn params_cargo_run() -> serde_json::Value {
    tool_parameters_schema_value::<CargoRunArgs>()
}

pub(in crate::tools) fn params_rust_test_one() -> serde_json::Value {
    tool_parameters_schema_value::<RustTestOneArgs>()
}

pub(in crate::tools) fn params_cargo_metadata() -> serde_json::Value {
    tool_parameters_schema_value::<CargoMetadataArgs>()
}

pub(in crate::tools) fn params_cargo_tree() -> serde_json::Value {
    tool_parameters_schema_value::<CargoTreeArgs>()
}

pub(in crate::tools) fn params_cargo_clean() -> serde_json::Value {
    tool_parameters_schema_value::<CargoCleanArgs>()
}

pub(in crate::tools) fn params_cargo_doc() -> serde_json::Value {
    tool_parameters_schema_value::<CargoDocArgs>()
}

pub(in crate::tools) fn params_cargo_nextest() -> serde_json::Value {
    tool_parameters_schema_value::<CargoNextestArgs>()
}

pub(in crate::tools) fn params_cargo_fmt_check() -> serde_json::Value {
    tool_parameters_schema_value::<EmptyToolArgs>()
}

pub(in crate::tools) fn params_cargo_outdated() -> serde_json::Value {
    tool_parameters_schema_value::<CargoOutdatedArgs>()
}

pub(in crate::tools) fn params_cargo_machete() -> serde_json::Value {
    tool_parameters_schema_value::<CargoMacheteArgs>()
}

pub(in crate::tools) fn params_cargo_udeps() -> serde_json::Value {
    tool_parameters_schema_value::<CargoUdepsArgs>()
}

pub(in crate::tools) fn params_cargo_publish_dry_run() -> serde_json::Value {
    tool_parameters_schema_value::<CargoPublishDryRunArgs>()
}

pub(in crate::tools) fn params_rust_rustc() -> serde_json::Value {
    tool_parameters_schema_value::<RustRustcArgs>()
}

pub(in crate::tools) fn params_rust_compiler_json() -> serde_json::Value {
    tool_parameters_schema_value::<RustCompilerJsonArgs>()
}

pub(in crate::tools) fn params_rust_analyzer_position() -> serde_json::Value {
    tool_parameters_schema_value::<RustAnalyzerPositionArgs>()
}

pub(in crate::tools) fn params_rust_analyzer_references() -> serde_json::Value {
    tool_parameters_schema_value::<RustAnalyzerReferencesArgs>()
}

pub(in crate::tools) fn params_rust_analyzer_hover() -> serde_json::Value {
    tool_parameters_schema_value::<RustAnalyzerPositionArgs>()
}

pub(in crate::tools) fn params_rust_analyzer_document_symbol() -> serde_json::Value {
    tool_parameters_schema_value::<RustAnalyzerDocumentSymbolArgs>()
}

pub(in crate::tools) fn params_rust_analyzer_workspace_symbol() -> serde_json::Value {
    tool_parameters_schema_value::<RustAnalyzerWorkspaceSymbolArgs>()
}

pub(in crate::tools) fn params_cargo_fix() -> serde_json::Value {
    tool_parameters_schema_value::<CargoFixArgs>()
}

pub(in crate::tools) fn params_cargo_audit() -> serde_json::Value {
    tool_parameters_schema_value::<CargoAuditArgs>()
}

pub(in crate::tools) fn params_cargo_deny() -> serde_json::Value {
    tool_parameters_schema_value::<CargoDenyArgs>()
}

pub(in crate::tools) fn params_rust_file_outline() -> serde_json::Value {
    tool_parameters_schema_value::<RustFileOutlineArgs>()
}
