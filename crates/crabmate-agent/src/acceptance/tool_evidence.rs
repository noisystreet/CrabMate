//! 从 `role: tool` 消息对 `PlanStepAcceptance` 执行验收（证据生命周期限于本函数）。

use crabmate_tools::tool_result;
use crabmate_types::{Message, message_content_as_str};

use crate::plan_artifact::PlanStepAcceptance;

use super::{AcceptanceSpec, VerifyOutcome, verify_against_spec};

/// 单条 `role: tool` 消息是否满足 `acceptance`（分阶段步窗口聚合时逐条调用）。
pub fn verify_plan_step_acceptance_for_tool_message(
    acceptance: &PlanStepAcceptance,
    tool_msg: &Message,
    workspace_root: &std::path::Path,
) -> VerifyOutcome {
    let spec = AcceptanceSpec::from(acceptance);
    let tool_name = tool_msg.name.as_deref().unwrap_or("");
    let tool_output = message_content_as_str(&tool_msg.content).unwrap_or("");
    let parsed = tool_result::parse_legacy_output(tool_name, tool_output);
    let tool_error_opt = parsed.exit_code.map(|code| tool_result::ToolError {
        code: code.to_string(),
        category: tool_result::ToolFailureCategory::External,
        message: "Verification fake error".to_string(),
        legacy_parsed: parsed.clone(),
        retryable: false,
    });
    let ev = super::AcceptanceEvidence {
        tool_name,
        tool_output,
        stdout: parsed.stdout.as_str(),
        stderr: parsed.stderr.as_str(),
        tool_error: tool_error_opt.as_ref(),
        fallback_exit_code: None,
        workspace_root,
        file_resolve: spec.file_resolve,
        combined_text_override: None,
    };
    verify_against_spec(&spec, &ev)
}
