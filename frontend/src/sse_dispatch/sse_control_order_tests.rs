use std::fs;
use std::path::PathBuf;

use crabmate_sse_protocol::classify_sse_control_outcome;
use serde_json::Value;

use crate::i18n::Locale;

use super::{
    SseClarifyTraceHooks, SseControlSink, SseDispatch, SseNoticeTimelineHooks, SseStagedPlanHooks,
    SseWorkspaceToolHooks, try_dispatch_sse_control_payload,
};

#[test]
fn single_space_sse_payload_is_plain_not_handled() {
    assert_eq!(dispatch_triage_string(" "), "plain");
}

#[test]
fn standalone_protocol_version_frame_is_handled_not_plain() {
    assert_eq!(dispatch_triage_string(r#"{"v":1}"#), "handled");
}

fn dispatch_triage_string(data: &str) -> &'static str {
    let mut on_err = |_msg: String| {};
    let mut sink = SseControlSink {
        user_locale: Locale::ZhHans,
        on_error: &mut on_err,
        on_delta: None,
        workspace_tool: SseWorkspaceToolHooks {
            on_workspace_changed: None,
            on_tool_call: None,
            on_tool_status_change: None,
            on_parsing_tool_calls_change: None,
            on_tool_output_chunk: None,
            on_tool_result: None,
            on_command_approval_request: None,
        },
        staged_plan: SseStagedPlanHooks {
            on_assistant_answer_phase: None,
            on_staged_plan_step_started: None,
            on_staged_plan_step_finished: None,
            on_turn_segment_start: None,
            on_turn_segment_end: None,
            on_turn_tool_phase_end: None,
        },
        clarify_trace: SseClarifyTraceHooks {
            on_clarification_questionnaire: None,
            on_thinking_trace: None,
        },
        notice_timeline: SseNoticeTimelineHooks {
            on_conversation_saved_revision: None,
            on_timeline_log: None,
            on_run_finished: None,
            on_state_snapshot: None,
        },
    };
    match try_dispatch_sse_control_payload(data, &mut sink) {
        SseDispatch::Stop => "stop",
        SseDispatch::Handled => "handled",
        SseDispatch::Plain => "plain",
        SseDispatch::StreamEnded => "stream_ended",
    }
}

/// 与共享 `classify_sse_control_outcome` 一致；与金样一致（`sse_capabilities` 版本不匹配时
/// `try_dispatch` 可能额外 `Stop`，金样不覆盖该情形）。
#[test]
fn golden_sse_control_leptos_dispatch_matches_shared_classify() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("../fixtures/sse_control_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = t.splitn(3, '\t').collect();
        assert!(
            parts.len() == 3,
            "{}:{}: expected 3 tab columns",
            path.display(),
            line_no + 1
        );
        let json_line = parts[1].trim();
        let want = parts[2].trim();
        let v: Value = serde_json::from_str(json_line).unwrap_or_else(|e| {
            panic!(
                "{}:{}: invalid json: {e}\n{json_line}",
                path.display(),
                line_no + 1
            )
        });
        let via_classify = classify_sse_control_outcome(&v);
        let via_dispatch = dispatch_triage_string(json_line);
        assert_eq!(
            via_classify,
            via_dispatch,
            "{}:{}: Leptos `try_dispatch` triage must match `crabmate-sse-protocol::classify_sse_control_outcome`\n  json: {json_line}",
            path.display(),
            line_no + 1
        );
        assert_eq!(
            via_dispatch,
            want,
            "{}:{}: dispatch triage must match golden fixture\n  json: {json_line}",
            path.display(),
            line_no + 1
        );
    }
}
