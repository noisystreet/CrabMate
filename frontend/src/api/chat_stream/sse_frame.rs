use crabmate_sse_protocol::{
    StreamEndReason, extract_stream_ended_reason, is_sse_done_sentinel, join_sse_data_lines,
    parse_sse_event_id,
};

use crate::i18n::Locale;
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, SseClarifyTraceHooks, SseControlSink,
    SseNoticeTimelineHooks, SseStagedPlanHooks, SseWorkspaceToolHooks, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, ThinkingTraceInfo, TimelineLogInfo, ToolOutputChunkInfo,
    ToolResultInfo, try_dispatch_sse_control_payload,
};

use super::ChatStreamCallbacks;

pub(super) fn process_sse_buffer(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<usize, String> {
    let mut meaningful = 0usize;
    while let Some(pos) = buffer.find("\n\n") {
        let block = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();
        if handle_sse_block(&block, last_event_id, saw_stream_ended, cbs, loc)? {
            meaningful = meaningful.saturating_add(1);
        }
    }
    Ok(meaningful)
}

pub(super) fn flush_sse_tail(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<usize, String> {
    // 勿对尾部缓冲 `trim`：流式正文可能单独落在仅含空格/`data: ` 尾部的帧里，trim 会吞掉词间空格。
    let meaningful = if buffer.is_empty() {
        0usize
    } else if handle_sse_block(buffer.as_str(), last_event_id, saw_stream_ended, cbs, loc)? {
        1
    } else {
        0
    };
    buffer.clear();
    Ok(meaningful)
}

/// `Ok(true)`：本帧带有非空、非 `[DONE]` 的 `data:` 负载，并已走完 `stream_ended` 或控制面/正文分发。
pub(super) fn handle_sse_block(
    block: &str,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<bool, String> {
    if let Some(id) = parse_sse_event_id(block) {
        *last_event_id = id;
        (cbs.on_last_sse_event_id)(id);
    }
    let Some(data) = join_sse_data_lines(block) else {
        return Ok(false);
    };
    // 勿对 `data` 全文 `trim`：模型/代理可能把词间空格单独打成一段 SSE，trim 会导致单词粘在一起。
    if data.is_empty() || is_sse_done_sentinel(&data) {
        return Ok(false);
    }
    if let Some(reason) = extract_stream_ended_reason(&data) {
        *saw_stream_ended = true;
        (cbs.on_stream_ended)(reason);
        return Ok(true);
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data)
        && let Some(ended) = v.get("stream_ended")
        && !ended.is_null()
    {
        // `reason` 缺失或非字符串时仍须回落 busy（与 `dispatch_notice_timeline_tail` 吞掉 `stream_ended` 的形态对齐）。
        let reason = ended
            .get("reason")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| StreamEndReason::Completed.to_string());
        *saw_stream_ended = true;
        (cbs.on_stream_ended)(reason);
        return Ok(true);
    }

    let mut stop = false;
    let mut on_err = |msg: String| {
        stop = true;
        (cbs.on_error)(msg);
    };
    let mut on_ws = || (cbs.on_workspace_changed)();
    let mut on_tool_call = |n: String,
                            s: String,
                            p: Option<String>,
                            a: Option<String>,
                            g: Option<String>,
                            tid: Option<String>| {
        (cbs.on_tool_call)(n, s, p, a, g, tid);
    };
    let mut on_tool_status = |b: bool| (cbs.on_tool_status)(b);
    let mut on_parse = |_b: bool| {};
    let mut on_tool_chunk = |info: ToolOutputChunkInfo| (cbs.on_tool_output_chunk)(info);
    let mut on_tool_res = |info: ToolResultInfo| (cbs.on_tool_result)(info);
    let mut on_appr = |req: CommandApprovalRequest| (cbs.on_approval)(req);
    let mut on_conv_rev = |rev: u64| (cbs.on_conversation_revision)(rev);
    let mut on_staged_start =
        |info: StagedPlanStepStartInfo| (cbs.on_staged_plan_step_started)(info);
    let mut on_staged_end = |info: StagedPlanStepEndInfo| (cbs.on_staged_plan_step_finished)(info);
    let mut on_clar =
        |info: ClarificationQuestionnaireInfo| (cbs.on_clarification_questionnaire)(info);
    let mut on_phase = || (cbs.on_assistant_answer_phase)();
    let mut on_thinking_trace = |info: ThinkingTraceInfo| (cbs.on_thinking_trace)(info);
    let mut on_timeline_log = |info: TimelineLogInfo| (cbs.on_timeline_log)(info);

    let mut cbs2 = SseControlSink {
        user_locale: loc,
        on_error: &mut on_err,
        workspace_tool: SseWorkspaceToolHooks {
            on_workspace_changed: Some(&mut on_ws),
            on_tool_call: Some(&mut on_tool_call),
            on_tool_status_change: Some(&mut on_tool_status),
            on_parsing_tool_calls_change: Some(&mut on_parse),
            on_tool_output_chunk: Some(&mut on_tool_chunk),
            on_tool_result: Some(&mut on_tool_res),
            on_command_approval_request: Some(&mut on_appr),
        },
        staged_plan: SseStagedPlanHooks {
            on_assistant_answer_phase: Some(&mut on_phase),
            on_staged_plan_step_started: Some(&mut on_staged_start),
            on_staged_plan_step_finished: Some(&mut on_staged_end),
        },
        clarify_trace: SseClarifyTraceHooks {
            on_clarification_questionnaire: Some(&mut on_clar),
            on_thinking_trace: Some(&mut on_thinking_trace),
        },
        notice_timeline: SseNoticeTimelineHooks {
            on_conversation_saved_revision: Some(&mut on_conv_rev),
            on_timeline_log: Some(&mut on_timeline_log),
        },
    };
    match try_dispatch_sse_control_payload(&data, &mut cbs2) {
        crate::sse_dispatch::SseDispatch::Stop => Ok(true),
        crate::sse_dispatch::SseDispatch::Handled => {
            if stop {
                Err(crate::i18n::api_err_stream_stopped(loc).to_string())
            } else {
                Ok(true)
            }
        }
        crate::sse_dispatch::SseDispatch::Plain => {
            if stop {
                return Err(crate::i18n::api_err_stream_stopped(loc).to_string());
            }
            (cbs.on_delta)(data);
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ChatStreamCallbacks;
    use super::{flush_sse_tail, handle_sse_block, process_sse_buffer};
    use crate::i18n::Locale;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn callbacks_with_end_capture(ended: Rc<RefCell<Option<String>>>) -> ChatStreamCallbacks {
        ChatStreamCallbacks {
            on_delta: Rc::new(|_s| {}),
            on_done: Rc::new(|| {}),
            on_error: Rc::new(|_e| {}),
            on_workspace_changed: Rc::new(|| {}),
            on_tool_status: Rc::new(|_b| {}),
            on_tool_output_chunk: Rc::new(|_info| {}),
            on_tool_result: Rc::new(|_info| {}),
            on_approval: Rc::new(|_req| {}),
            on_conversation_id: Rc::new(|_id| {}),
            on_conversation_revision: Rc::new(|_rev| {}),
            on_stream_ended: Rc::new(move |reason| {
                *ended.borrow_mut() = Some(reason);
            }),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
        }
    }

    #[test]
    fn handle_block_marks_stream_ended_when_reason_present() {
        let ended = Rc::new(RefCell::new(None::<String>));
        let cbs = callbacks_with_end_capture(Rc::clone(&ended));
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "id: 12\ndata: {\"stream_ended\":{\"job_id\":1,\"reason\":\"completed\"}}\n\n";
        let res = handle_sse_block(
            block.trim(),
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert!(saw_stream_ended);
        assert_eq!(last_event_id, 12);
        assert_eq!(ended.borrow().as_deref(), Some("completed"));
    }

    #[test]
    fn handle_block_marks_stream_ended_when_reason_missing_uses_completed() {
        let ended = Rc::new(RefCell::new(None::<String>));
        let cbs = callbacks_with_end_capture(Rc::clone(&ended));
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "data: {\"stream_ended\":{\"job_id\":3}}\n\n";
        let res = handle_sse_block(
            block.trim(),
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert!(saw_stream_ended);
        assert_eq!(ended.borrow().as_deref(), Some("completed"));
    }

    /// `data: ` 后仅空格的增量不得被 `trim_start` 吞掉，否则英文词会粘在一起。
    #[test]
    fn handle_block_preserves_whitespace_only_delta() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            on_done: Rc::new(|| {}),
            on_error: Rc::new(|_e| {}),
            on_workspace_changed: Rc::new(|| {}),
            on_tool_status: Rc::new(|_b| {}),
            on_tool_output_chunk: Rc::new(|_info| {}),
            on_tool_result: Rc::new(|_info| {}),
            on_approval: Rc::new(|_req| {}),
            on_conversation_id: Rc::new(|_id| {}),
            on_conversation_revision: Rc::new(|_rev| {}),
            on_stream_ended: Rc::new(|_reason| {}),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
        };
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "data:  \n\n";
        let res = handle_sse_block(
            block,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert_eq!(got.borrow().as_str(), " ");
    }

    /// process_sse_buffer: 空 buffer 返回 0。
    #[test]
    fn process_sse_buffer_empty() {
        let cbs = callbacks_with_end_capture(Rc::new(RefCell::new(None)));
        let mut buf = String::new();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = process_sse_buffer(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    /// process_sse_buffer: 无 `\n\n` 分隔符时返回 0，buffer 不变。
    #[test]
    fn process_sse_buffer_no_delimiter() {
        let cbs = callbacks_with_end_capture(Rc::new(RefCell::new(None)));
        let mut buf = "data: hello".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = process_sse_buffer(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 0);
        assert_eq!(buf, "data: hello");
    }

    /// process_sse_buffer: 单个完整 SSE 块。
    #[test]
    fn process_sse_buffer_single_block() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            ..callbacks_with_end_capture(Rc::new(RefCell::new(None)))
        };
        let mut buf = "data: hello\n\n".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = process_sse_buffer(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert!(buf.is_empty());
        assert_eq!(got.borrow().as_str(), "hello");
    }

    /// process_sse_buffer: 多个 SSE 块一次处理。
    #[test]
    fn process_sse_buffer_multiple_blocks() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            ..callbacks_with_end_capture(Rc::new(RefCell::new(None)))
        };
        let mut buf = "data: a\n\ndata: b\n\n".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = process_sse_buffer(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 2);
        assert!(buf.is_empty());
        assert_eq!(got.borrow().as_str(), "ab");
    }

    /// process_sse_buffer: 块后残留未完成数据。
    #[test]
    fn process_sse_buffer_with_tail() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            ..callbacks_with_end_capture(Rc::new(RefCell::new(None)))
        };
        let mut buf = "data: hello\n\ndata: wor".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = process_sse_buffer(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf, "data: wor");
        assert_eq!(got.borrow().as_str(), "hello");
    }

    /// flush_sse_tail: 空 buffer 返回 0。
    #[test]
    fn flush_sse_tail_empty() {
        let cbs = callbacks_with_end_capture(Rc::new(RefCell::new(None)));
        let mut buf = String::new();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = flush_sse_tail(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    /// flush_sse_tail: 尾部有完整 SSE 块（无 `\n\n` 后缀）。
    #[test]
    fn flush_sse_tail_meaningful() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            ..callbacks_with_end_capture(Rc::new(RefCell::new(None)))
        };
        let mut buf = "data: tail".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = flush_sse_tail(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert!(buf.is_empty());
        assert_eq!(got.borrow().as_str(), "tail");
    }

    /// flush_sse_tail: 尾部仅空白（`data:  `），不应被 trim 吞掉。
    #[test]
    fn flush_sse_tail_whitespace_only() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            ..callbacks_with_end_capture(Rc::new(RefCell::new(None)))
        };
        let mut buf = "data:  ".to_string();
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let n = flush_sse_tail(
            &mut buf,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert!(buf.is_empty());
        assert_eq!(got.borrow().as_str(), " ");
    }
}
