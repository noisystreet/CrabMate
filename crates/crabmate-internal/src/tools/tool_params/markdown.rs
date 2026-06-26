//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::MarkdownCheckLinksArgs;

pub(in crate::tools) fn params_markdown_check_links() -> serde_json::Value {
    tool_parameters_schema_value::<MarkdownCheckLinksArgs>()
}
