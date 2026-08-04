//! 工具结果卡片：薄封装，正文在共享 crate [`crabmate_tool_card`]。

use crabmate_tool_card::ToolCardInput;

use crate::sse_dispatch::ToolResultInfo;

#[must_use]
pub fn tool_result_to_card_input(info: &ToolResultInfo) -> ToolCardInput {
    ToolCardInput {
        name: info.name.clone(),
        goal_id: info.goal_id.clone(),
        tool_call_id: info.tool_call_id.clone(),
        result_version: info.result_version,
        summary: info.summary.clone(),
        output: info.output.clone(),
        ok: info.ok,
        exit_code: info.exit_code,
        error_code: info.error_code.clone(),
        failure_category: info.failure_category.clone(),
        structured_preview: info.structured_preview.clone(),
    }
}
