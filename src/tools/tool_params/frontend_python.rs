//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    FrontendLintArgs, MypyCheckArgs, PytestRunArgs, PythonInstallEditableArgs,
    PythonSnippetRunArgs, RuffCheckArgs, UvRunArgs, UvSyncArgs,
};

pub(in crate::tools) fn params_frontend_lint() -> serde_json::Value {
    tool_parameters_schema_value::<FrontendLintArgs>()
}

pub(in crate::tools) fn params_ruff_check() -> serde_json::Value {
    tool_parameters_schema_value::<RuffCheckArgs>()
}

pub(in crate::tools) fn params_pytest_run() -> serde_json::Value {
    tool_parameters_schema_value::<PytestRunArgs>()
}

pub(in crate::tools) fn params_mypy_check() -> serde_json::Value {
    tool_parameters_schema_value::<MypyCheckArgs>()
}

pub(in crate::tools) fn params_python_install_editable() -> serde_json::Value {
    tool_parameters_schema_value::<PythonInstallEditableArgs>()
}

pub(in crate::tools) fn params_uv_sync() -> serde_json::Value {
    tool_parameters_schema_value::<UvSyncArgs>()
}

pub(in crate::tools) fn params_uv_run() -> serde_json::Value {
    tool_parameters_schema_value::<UvRunArgs>()
}

pub(in crate::tools) fn params_python_snippet_run() -> serde_json::Value {
    tool_parameters_schema_value::<PythonSnippetRunArgs>()
}
