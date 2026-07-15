use crabmate_sse_protocol::{
    StreamEndReason, extract_stream_ended_reason, is_sse_done_sentinel, join_sse_data_lines,
    parse_sse_event_id,
};

use crate::i18n::Locale;
use crate::sse_dispatch::{
    SseClarifyTraceHooks, SseControlSink, SseNoticeTimelineHooks, SseStagedPlanHooks,
    SseWorkspaceToolHooks,
};

use super::ChatStreamCallbacks;
use super::sse_parser::{SseParser, V1Parser};

/// 当前使用的 SSE 解析器（v1）。Phase 2 将改为按 `client_sse_protocol` 选择。
fn default_parser() -> &'static dyn SseParser {
    &V1Parser
}

/// SSE 单帧分类：用于区分「任意有效负载」与「正文 delta」的空闲检测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SseFrameKind {
    Ignored,
    Control,
    TextDelta,
    StreamEnded,
}

impl SseFrameKind {
    #[must_use]
    pub(super) fn counts_as_meaningful(self) -> bool {
        !matches!(self, Self::Ignored)
    }

    #[must_use]
    pub(super) fn counts_as_text_delta(self) -> bool {
        matches!(self, Self::TextDelta)
    }
}

/// `process_sse_buffer` / `flush_sse_tail` 累计进度。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SseBufferProgress {
    pub meaningful: usize,
    pub text_deltas: usize,
}

impl SseBufferProgress {
    fn absorb_frame(&mut self, kind: SseFrameKind) {
        if kind.counts_as_meaningful() {
            self.meaningful = self.meaningful.saturating_add(1);
        }
        if kind.counts_as_text_delta() {
            self.text_deltas = self.text_deltas.saturating_add(1);
        }
    }
}

fn stream_ended_tiktoken_from_data(
    data: &str,
) -> Option<crate::conversation_hydrate::TiktokenPromptTokensSnapshot> {
    let v = serde_json::from_str::<serde_json::Value>(data).ok()?;
    v.get("stream_ended").and_then(|ended| {
        ended
            .get("tiktoken_prompt_tokens")
            .and_then(crate::conversation_prompt_tokens_apply::parse_tiktoken_prompt_tokens_value)
    })
}

#[allow(dead_code)]
pub(super) fn process_sse_buffer(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<SseBufferProgress, String> {
    let mut progress = SseBufferProgress::default();
    while let Some(step) =
        process_sse_buffer_step(buffer, last_event_id, saw_stream_ended, cbs, loc)?
    {
        progress.absorb_frame(step);
    }
    Ok(progress)
}

/// 至多解析并分发一帧 SSE（`\\n\\n` 分隔）；无完整帧时返回 `None`。
pub(super) fn process_sse_buffer_step(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<Option<SseFrameKind>, String> {
    let Some(pos) = buffer.find("\n\n") else {
        return Ok(None);
    };
    let block = buffer[..pos].to_string();
    *buffer = buffer[pos + 2..].to_string();
    handle_sse_block(&block, last_event_id, saw_stream_ended, cbs, loc).map(Some)
}

pub(super) fn flush_sse_tail(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<SseBufferProgress, String> {
    // 勿对尾部缓冲 `trim`：流式正文可能单独落在仅含空格/`data: ` 尾部的帧里，trim 会吞掉词间空格。
    let mut progress = SseBufferProgress::default();
    if !buffer.is_empty() {
        let kind = handle_sse_block(buffer.as_str(), last_event_id, saw_stream_ended, cbs, loc)?;
        progress.absorb_frame(kind);
    }
    buffer.clear();
    Ok(progress)
}

/// 单帧 SSE 块解析与分发。
pub(super) fn handle_sse_block(
    block: &str,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<SseFrameKind, String> {
    if let Some(id) = parse_sse_event_id(block) {
        *last_event_id = id;
        (cbs.on_last_sse_event_id)(id);
    }
    let Some(data) = join_sse_data_lines(block) else {
        return Ok(SseFrameKind::Ignored);
    };
    // 勿对 `data` 全文 `trim`：模型/代理可能把词间空格单独打成一段 SSE，trim 会导致单词粘在一起。
    if data.is_empty() || is_sse_done_sentinel(&data) {
        return Ok(SseFrameKind::Ignored);
    }
    if let Some(reason) = extract_stream_ended_reason(&data) {
        *saw_stream_ended = true;
        let tiktoken = stream_ended_tiktoken_from_data(&data);
        (cbs.on_stream_ended)(reason, tiktoken);
        return Ok(SseFrameKind::StreamEnded);
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
        let tiktoken = ended
            .get("tiktoken_prompt_tokens")
            .and_then(crate::conversation_prompt_tokens_apply::parse_tiktoken_prompt_tokens_value);
        (cbs.on_stream_ended)(reason, tiktoken);
        return Ok(SseFrameKind::StreamEnded);
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
    let mut on_parse = |b: bool| (cbs.on_parsing_tool_calls)(b);
    let mut on_tool_chunk = |info| (cbs.on_tool_output_chunk)(info);
    let mut on_tool_res = |info| (cbs.on_tool_result)(info);
    let mut on_appr = |req| (cbs.on_approval)(req);
    let mut on_conv_rev =
        |rev: u64, tiktoken: Option<crate::conversation_hydrate::TiktokenPromptTokensSnapshot>| {
            (cbs.on_conversation_revision)(rev, tiktoken);
        };
    let mut on_staged_start = |info| (cbs.on_staged_plan_step_started)(info);
    let mut on_staged_end = |info| (cbs.on_staged_plan_step_finished)(info);
    let mut on_clar = |info| (cbs.on_clarification_questionnaire)(info);
    let mut on_phase = || (cbs.on_assistant_answer_phase)();
    let mut on_turn_seg_start = |info: crate::sse_dispatch::TurnSegmentStartInfo| {
        (cbs.on_turn_segment_start)(info);
    };
    let mut on_turn_seg_end = |segment_id: String| (cbs.on_turn_segment_end)(segment_id);
    let mut on_turn_phase_end = || (cbs.on_turn_tool_phase_end)();
    let mut on_thinking_trace = |info| (cbs.on_thinking_trace)(info);
    let mut on_timeline_log = |info| (cbs.on_timeline_log)(info);

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
            on_turn_segment_start: Some(&mut on_turn_seg_start),
            on_turn_segment_end: Some(&mut on_turn_seg_end),
            on_turn_tool_phase_end: Some(&mut on_turn_phase_end),
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
    match default_parser().parse(&data, &mut cbs2) {
        crate::sse_dispatch::SseDispatch::Stop => Ok(SseFrameKind::Control),
        crate::sse_dispatch::SseDispatch::Handled => {
            if stop {
                Err(crate::i18n::api_err_stream_stopped(loc).to_string())
            } else {
                Ok(SseFrameKind::Control)
            }
        }
        crate::sse_dispatch::SseDispatch::Plain => {
            if stop {
                return Err(crate::i18n::api_err_stream_stopped(loc).to_string());
            }
            (cbs.on_delta)(data);
            Ok(SseFrameKind::TextDelta)
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
            on_conversation_revision: Rc::new(|_rev, _tik| {}),
            on_stream_ended: Rc::new(move |reason, _tik| {
                *ended.borrow_mut() = Some(reason);
            }),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_parsing_tool_calls: Rc::new(|_b| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
            on_turn_segment_start: Rc::new(|_info| {}),
            on_turn_segment_end: Rc::new(|_id| {}),
            on_turn_tool_phase_end: Rc::new(|| {}),
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
            on_conversation_revision: Rc::new(|_rev, _tik| {}),
            on_stream_ended: Rc::new(|_reason, _tik| {}),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_parsing_tool_calls: Rc::new(|_b| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
            on_turn_segment_start: Rc::new(|_info| {}),
            on_turn_segment_end: Rc::new(|_id| {}),
            on_turn_tool_phase_end: Rc::new(|| {}),
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
        assert_eq!(n.meaningful, 0);
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
        assert_eq!(n.meaningful, 0);
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
        assert_eq!(n.meaningful, 1);
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
        assert_eq!(n.meaningful, 2);
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
        assert_eq!(n.meaningful, 1);
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
        assert_eq!(n.meaningful, 0);
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
        assert_eq!(n.meaningful, 1);
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
        assert_eq!(n.meaningful, 1);
        assert!(buf.is_empty());
        assert_eq!(got.borrow().as_str(), " ");
    }

    #[test]
    fn timeline_log_counts_meaningful_not_text_delta() {
        let cbs = callbacks_with_end_capture(Rc::new(RefCell::new(None)));
        let mut buf = String::from(
            "data: {\"timeline_log\":{\"kind\":\"intent_analysis\",\"title\":\"意图分析：问答类\"}}\n\n",
        );
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
        assert_eq!(n.meaningful, 1);
        assert_eq!(n.text_deltas, 0);
    }
}
