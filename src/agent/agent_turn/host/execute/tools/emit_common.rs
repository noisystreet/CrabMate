//! 工具结果 SSE 信封规范化（并行/串行批共用）。

use crate::sse::{SseEncoder, SsePayload, ToolResultBody, send_sse_control_payload_optional};
use crate::tool_result::{self, NormalizedToolEnvelope, ToolEnvelopeContext, parse_legacy_output};
use crate::tools;

/// 按执行模式构造 [`ToolEnvelopeContext`]。
pub(super) fn tool_envelope_context<'a>(
    tool_call_id: &'a str,
    execution_mode: &'static str,
    parallel_batch_id: Option<&'a str>,
) -> ToolEnvelopeContext<'a> {
    ToolEnvelopeContext {
        tool_call_id,
        execution_mode,
        parallel_batch_id,
    }
}

/// 解析工具入参并生成摘要（失败时回退到原始 args 字符串摘要）。
pub(super) fn summarize_tool_call_args(name: &str, args: &str) -> Option<String> {
    match tools::parse_args_json(args) {
        Ok(parsed) => tools::summarize_tool_call_parsed(name, &parsed),
        Err(_) => tools::summarize_tool_call(name, args),
    }
}

/// SSE：`SsePayload::ToolResult`（含 stdout/stderr、retryable、信封元数据）。
/// `reflection_inject` 中若含 `tool_job` 对象（`run_command` 的 `async=true` 启动帧），
/// 提取为 `tool_result.tool_job_*` 软字段（契约 §2）。
#[allow(clippy::too_many_arguments)] // 工具结果 SSE 组装：SSE 通道、工具元数据与注入帧
pub(super) async fn emit_sse_tool_result(
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    name: &str,
    result: &str,
    tool_summary: Option<String>,
    envelope_ctx: Option<ToolEnvelopeContext<'_>>,
    reflection_inject: Option<&serde_json::Value>,
    encoder: &dyn SseEncoder,
) {
    let parsed = parse_legacy_output(name, result);
    let structured_payload = tool_result::structured_payload_for_tool(name, result);
    let summary_for_norm = tool_summary
        .clone()
        .unwrap_or_else(|| format!("tool: {name}"));
    let norm = NormalizedToolEnvelope::from_tool_run(
        name,
        summary_for_norm,
        &parsed,
        result,
        envelope_ctx.as_ref(),
        structured_payload,
    );
    let mut structured_preview = crate::tools::structured_preview::structured_preview_for_tool_sse(
        name,
        result,
        norm.structured_payload.as_ref(),
    );
    if name == "run_command" {
        structured_preview =
            crate::tools::structured_preview::augment_run_command_preview_with_git_diff(
                structured_preview,
                result,
                parsed.stdout.as_str(),
            );
    }
    let stdout = if parsed.stdout.is_empty() {
        None
    } else {
        Some(parsed.stdout)
    };
    let stderr = if parsed.stderr.is_empty() {
        None
    } else {
        Some(parsed.stderr)
    };
    let tool_job = reflection_inject.and_then(|v| v.get("tool_job"));
    let tool_job_str = |key: &str| {
        tool_job
            .and_then(|j| j.get(key))
            .and_then(|x| x.as_str())
            .map(str::to_string)
    };
    let payload = SsePayload::ToolResult {
        tool_result: ToolResultBody {
            name: norm.name,
            goal_id: None,
            result_version: norm.envelope_version,
            summary: tool_summary,
            output: result.to_string(),
            ok: Some(norm.ok),
            exit_code: norm.exit_code,
            error_code: norm.error_code.clone(),
            failure_category: norm.failure_category.clone(),
            retryable: norm.retryable,
            tool_call_id: norm.tool_call_id,
            execution_mode: norm.execution_mode,
            parallel_batch_id: norm.parallel_batch_id,
            stdout,
            stderr,
            structured_preview,
            tool_job_id: tool_job_str("tool_job_id"),
            tool_job_poll_url: tool_job_str("tool_job_poll_url"),
            tool_job_status: tool_job_str("tool_job_status"),
        },
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::emit_tool_result_sse",
        encoder,
    )
    .await;
}
