//! 工具运行时呈现与统计的 `CM_*` 覆盖：`tool_transcript.*` / `agent_thinking_trace.*` /
//! `agent_tool_stats.*` / `thinking_echo.*` / `tool_call_explain.*`。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_bool, apply_nonempty_opt, apply_nonempty_opt_clearing_file, apply_parse,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    tool_transcript(b);
    thinking_trace(b);
    tool_stats(b);
    thinking_echo(b);
    tool_call_explain(b);
}

fn tool_transcript(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.tool_transcript.tool_message_max_chars,
        "CM_TOOL_MESSAGE_MAX_CHARS",
    );
    apply_bool(
        &mut b.tool_transcript.sse_tool_call_include_arguments,
        "CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS",
    );
    apply_bool(
        &mut b.tool_transcript.tool_result_envelope_v1,
        "CM_TOOL_RESULT_ENVELOPE_V1",
    );
}

fn thinking_trace(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.agent_thinking_trace.agent_thinking_trace_enabled,
        "CM_THINKING_TRACE_ENABLED",
    );
}

fn tool_stats(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.agent_tool_stats.agent_tool_stats_enabled,
        "CM_TOOL_STATS_ENABLED",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_window_events,
        "CM_TOOL_STATS_WINDOW_EVENTS",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_min_samples,
        "CM_TOOL_STATS_MIN_SAMPLES",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_max_chars,
        "CM_TOOL_STATS_MAX_CHARS",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_warn_below_success_ratio,
        "CM_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO",
    );
}

fn thinking_echo(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.thinking_echo.thinking_avoid_echo_system_prompt,
        "CM_THINKING_AVOID_ECHO_SYSTEM_PROMPT",
    );
    // 与 CM_SYSTEM_PROMPT_FILE 一致：后处理覆盖，故同时设置时文件优先于内联。
    apply_nonempty_opt_clearing_file(
        &mut b.thinking_echo.thinking_avoid_echo_appendix,
        &mut b.thinking_echo.thinking_avoid_echo_appendix_file,
        "CM_THINKING_AVOID_ECHO_APPENDIX",
    );
    apply_nonempty_opt(
        &mut b.thinking_echo.thinking_avoid_echo_appendix_file,
        "CM_THINKING_AVOID_ECHO_APPENDIX_FILE",
    );
}

fn tool_call_explain(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.tool_call_explain.tool_call_explain_enabled,
        "CM_TOOL_CALL_EXPLAIN_ENABLED",
    );
    apply_parse(
        &mut b.tool_call_explain.tool_call_explain_min_chars,
        "CM_TOOL_CALL_EXPLAIN_MIN_CHARS",
    );
    apply_parse(
        &mut b.tool_call_explain.tool_call_explain_max_chars,
        "CM_TOOL_CALL_EXPLAIN_MAX_CHARS",
    );
}
