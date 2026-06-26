//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    AstGrepRewriteArgs, AstGrepRunArgs, CodespellCheckArgs, PreCommitRunArgs, TyposCheckArgs,
};

pub(in crate::tools) fn params_pre_commit_run() -> serde_json::Value {
    tool_parameters_schema_value::<PreCommitRunArgs>()
}

pub(in crate::tools) fn params_typos_check() -> serde_json::Value {
    tool_parameters_schema_value::<TyposCheckArgs>()
}

pub(in crate::tools) fn params_codespell_check() -> serde_json::Value {
    tool_parameters_schema_value::<CodespellCheckArgs>()
}

pub(in crate::tools) fn params_ast_grep_run() -> serde_json::Value {
    tool_parameters_schema_value::<AstGrepRunArgs>()
}

pub(in crate::tools) fn params_ast_grep_rewrite() -> serde_json::Value {
    tool_parameters_schema_value::<AstGrepRewriteArgs>()
}
