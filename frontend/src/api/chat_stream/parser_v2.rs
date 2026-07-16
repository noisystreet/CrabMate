//! V2 解析器：将 AG-UI 协议 JSON 解析为控制面事件并分发到回调。
//!
//! 对应后端 `V2Encoder`：接收形如 `{"type":"EVENT_NAME",...}` 的 AG-UI 事件，
//! 映射到 `ChatStreamCallbacks` 的 `on_*` 回调。

use crate::sse_dispatch::{
    ClarificationFormField, ClarificationQuestionnaireInfo, CommandApprovalRequest, SseControlSink,
    SseDispatch, StagedPlanStepEndInfo, StagedPlanStepStartInfo, ThinkingTraceInfo,
    TimelineLogInfo, ToolOutputChunkInfo, ToolResultInfo, TurnSegmentStartInfo,
};

use super::sse_parser::SseParser;

/// V2 解析器（AG-UI 协议）。
pub(crate) struct V2Parser;

impl SseParser for V2Parser {
    fn parse(&self, data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch {
        parse_ag_ui_line(data, sink)
    }

    fn protocol_version(&self) -> u8 {
        2
    }
}

/// 解析单行 AG-UI JSON 事件并分发到 `SseControlSink` 回调。
fn parse_ag_ui_line(data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch {
    // 仅第一行可能是 V2 事件；多行时逐行处理（ToolCall 拆分可能产生 `\n` 拼接）。
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            // 不是合法 JSON → 按纯文本 delta 回落
            return SseDispatch::Plain;
        };
        let Some(type_str) = val.get("type").and_then(|v| v.as_str()) else {
            return SseDispatch::Plain;
        };
        match type_str {
            // ── 生命周期 ──
            "RUN_FINISHED" => {
                dispatch_run_finished(&val, sink);
                return SseDispatch::StreamEnded;
            }
            "RUN_ERROR" => {
                dispatch_run_error(&val, sink);
                return SseDispatch::StreamEnded;
            }

            // ── 工具调用 ──
            "TOOL_CALL_START" => dispatch_tool_call_start(&val, sink),
            "TOOL_CALL_ARGS" => dispatch_tool_call_args(&val, sink),
            "TOOL_CALL_END" => dispatch_tool_call_end(&val, sink),
            "TOOL_CALL_RESULT" => dispatch_tool_call_result(&val, sink),

            // ── CUSTOM 事件 ──
            "CUSTOM" => dispatch_custom(&val, sink),

            // 尚未实现的 AG-UI 标准事件 → Handled（不当作 Plain 回落）
            "RUN_STARTED"
            | "TEXT_MESSAGE_START"
            | "TEXT_MESSAGE_CONTENT"
            | "TEXT_MESSAGE_END"
            | "REASONING_MESSAGE_START"
            | "REASONING_MESSAGE_CONTENT"
            | "REASONING_MESSAGE_END"
            | "STATE_SNAPSHOT"
            | "STATE_DELTA" => {}

            // 未知 type → Plain 回落（可能是纯文本增量）
            _ => return SseDispatch::Plain,
        }
    }
    SseDispatch::Handled
}

// ── 生命周期 ──

fn dispatch_run_finished(_val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    // RUN_FINISHED → on_done
    if let Some(hook) = sink.notice_timeline.on_run_finished.as_mut() {
        hook();
    }
}

fn dispatch_run_error(val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    let msg = val
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("AG-UI run error");
    let code = val
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str());
    let line = match code {
        Some(c) => format!("{} ({})", msg, c),
        None => msg.to_string(),
    };
    (sink.on_error)(line);
    if let Some(hook) = sink.notice_timeline.on_run_finished.as_mut() {
        hook();
    }
}

// ── 工具调用 ──

fn dispatch_tool_call_start(val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    let tool_call_id = val.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("");
    let name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let summary = val.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = val
        .get("arguments")
        .and_then(|v| v.as_str())
        .or_else(|| val.get("argsPreview").and_then(|v| v.as_str()));
    let goal_id = val.get("goalId").and_then(|v| v.as_str());
    if let Some(hook) = sink.workspace_tool.on_tool_call.as_mut() {
        hook(
            name.to_string(),
            summary.to_string(),
            arguments.map(str::to_string),
            None, // args (full)
            goal_id.map(str::to_string),
            if tool_call_id.is_empty() {
                None
            } else {
                Some(tool_call_id.to_string())
            },
        );
    }
}

fn dispatch_tool_call_args(_val: &serde_json::Value, _sink: &mut SseControlSink<'_>) {
    // TOOL_CALL_ARGS: 当前前端无专用回调，可后续扩展
}

fn dispatch_tool_call_end(_val: &serde_json::Value, _sink: &mut SseControlSink<'_>) {
    // TOOL_CALL_END: 当前前端无专用回调，可后续扩展
}

fn dispatch_tool_call_result(val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    let tool_call_id = val.get("toolCallId").and_then(|v| v.as_str());
    let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let metadata = val.get("metadata");

    // 检查 partial 标记：若为 partial 输出片段，走 on_tool_output_chunk
    let is_partial = metadata
        .and_then(|m| m.get("partial"))
        .and_then(|p| p.as_bool())
        .unwrap_or(false);

    if is_partial {
        let chunk_info = ToolOutputChunkInfo {
            tool_call_id: tool_call_id.unwrap_or("").to_string(),
            name: metadata
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_string),
            seq: metadata
                .and_then(|m| m.get("seq"))
                .and_then(|s| s.as_u64())
                .unwrap_or(0),
            chunk: content.to_string(),
            stream: metadata
                .and_then(|m| m.get("stream"))
                .and_then(|s| s.as_str())
                .map(str::to_string),
        };
        if let Some(hook) = sink.workspace_tool.on_tool_output_chunk.as_mut() {
            hook(chunk_info);
        }
    } else {
        let result_info = ToolResultInfo {
            tool_call_id: tool_call_id.map(str::to_string),
            name: metadata
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
            goal_id: metadata
                .and_then(|m| m.get("goalId"))
                .and_then(|g| g.as_str())
                .map(str::to_string),
            output: content.to_string(),
            ok: metadata.and_then(|m| m.get("ok")).and_then(|o| o.as_bool()),
            summary: metadata
                .and_then(|m| m.get("summary"))
                .and_then(|s| s.as_str())
                .map(str::to_string),
            exit_code: metadata
                .and_then(|m| m.get("exitCode"))
                .and_then(|e| e.as_i64()),
            error_code: metadata
                .and_then(|m| m.get("errorCode"))
                .and_then(|e| e.as_str())
                .map(str::to_string),
            failure_category: metadata
                .and_then(|m| m.get("failureCategory"))
                .and_then(|f| f.as_str())
                .map(str::to_string),
            result_version: 1,
            structured_preview: None,
        };
        if let Some(hook) = sink.workspace_tool.on_tool_result.as_mut() {
            hook(result_info);
        }
    }
}

// ── CUSTOM 事件分发 ──

fn dispatch_custom(val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    let Some(custom_type) = val.get("customType").and_then(|v| v.as_str()) else {
        return;
    };
    match custom_type {
        "tool_running" | "parsing_tool_calls" | "workspace_changed" | "command_approval" => {
            dispatch_tool_custom(custom_type, val, sink);
        }
        "assistant_answer_phase"
        | "turn_segment_start"
        | "turn_segment_end"
        | "turn_tool_phase_end"
        | "staged_plan_step_started"
        | "staged_plan_step_finished" => {
            dispatch_plan_custom(custom_type, val, sink);
        }
        "clarification_questionnaire"
        | "thinking_trace"
        | "timeline_log"
        | "conversation_saved" => {
            dispatch_info_custom(custom_type, val, sink);
        }
        _ => {}
    }
}

/// 工具类 CUSTOM 事件分发。
fn dispatch_tool_custom(custom_type: &str, val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    match custom_type {
        "tool_running" => {
            let running = val
                .get("data")
                .and_then(|d| d.get("running"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(hook) = sink.workspace_tool.on_tool_status_change.as_mut() {
                hook(running);
            }
        }
        "parsing_tool_calls" => {
            let parsing = val
                .get("data")
                .and_then(|d| d.get("parsing"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(hook) = sink.workspace_tool.on_parsing_tool_calls_change.as_mut() {
                hook(parsing);
            }
        }
        "workspace_changed" => {
            if let Some(hook) = sink.workspace_tool.on_workspace_changed.as_mut() {
                hook();
            }
        }
        "command_approval" => {
            if let Some(data) = val.get("data") {
                let req = CommandApprovalRequest {
                    command: data
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    args: data
                        .get("args")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    allowlist_key: data
                        .get("allowlistKey")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.workspace_tool.on_command_approval_request.as_mut() {
                    hook(req);
                }
            }
        }
        _ => {}
    }
}

/// 阶段/规划类 CUSTOM 事件分发。
fn dispatch_plan_custom(custom_type: &str, val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    match custom_type {
        "assistant_answer_phase" => {
            if let Some(hook) = sink.staged_plan.on_assistant_answer_phase.as_mut() {
                hook();
            }
        }
        "turn_segment_start" => {
            if let Some(data) = val.get("data") {
                let info = TurnSegmentStartInfo {
                    segment_id: data
                        .get("segmentId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    kind: data
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    before_tool_call_id: data
                        .get("beforeToolCallId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.staged_plan.on_turn_segment_start.as_mut() {
                    hook(info);
                }
            }
        }
        "turn_segment_end" => {
            let segment_id = val
                .get("data")
                .and_then(|d| d.get("segmentId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(hook) = sink.staged_plan.on_turn_segment_end.as_mut() {
                hook(segment_id);
            }
        }
        "turn_tool_phase_end" => {
            if let Some(hook) = sink.staged_plan.on_turn_tool_phase_end.as_mut() {
                hook();
            }
        }
        "staged_plan_step_started" => {
            if let Some(data) = val.get("data") {
                let info = StagedPlanStepStartInfo {
                    step_index: data.get("stepIndex").and_then(|v| v.as_u64()).unwrap_or(0)
                        as usize,
                    total_steps: data.get("totalSteps").and_then(|v| v.as_u64()).unwrap_or(0)
                        as usize,
                    description: data
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    executor_kind: data
                        .get("executorKind")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.staged_plan.on_staged_plan_step_started.as_mut() {
                    hook(info);
                }
            }
        }
        "staged_plan_step_finished" => {
            if let Some(data) = val.get("data") {
                let info = StagedPlanStepEndInfo {
                    step_index: data.get("stepIndex").and_then(|v| v.as_u64()).unwrap_or(0)
                        as usize,
                    total_steps: data.get("totalSteps").and_then(|v| v.as_u64()).unwrap_or(0)
                        as usize,
                    status: data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    executor_kind: data
                        .get("executorKind")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.staged_plan.on_staged_plan_step_finished.as_mut() {
                    hook(info);
                }
            }
        }
        _ => {}
    }
}

/// 信息类 CUSTOM 事件分发（澄清问卷、思维迹、时间线旁注、会话保存）。
fn dispatch_info_custom(custom_type: &str, val: &serde_json::Value, sink: &mut SseControlSink<'_>) {
    match custom_type {
        "clarification_questionnaire" => {
            if let Some(data) = val.get("data") {
                let fields: Vec<ClarificationFormField> = data
                    .get("questions")
                    .and_then(|q| q.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|f| {
                                Some(ClarificationFormField {
                                    id: f.get("id")?.as_str()?.to_string(),
                                    label: f.get("label")?.as_str()?.to_string(),
                                    hint: f.get("hint")?.as_str().map(str::to_string),
                                    required: f
                                        .get("required")
                                        .and_then(|r| r.as_bool())
                                        .unwrap_or(false),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let info = ClarificationQuestionnaireInfo {
                    questionnaire_id: data
                        .get("questionnaireId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    intro: data
                        .get("intro")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    fields,
                };
                if let Some(hook) = sink.clarify_trace.on_clarification_questionnaire.as_mut() {
                    hook(info);
                }
            }
        }
        "thinking_trace" => {
            if let Some(data) = val.get("data") {
                let info = ThinkingTraceInfo {
                    op: data
                        .get("op")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    node_id: data
                        .get("nodeId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    parent_id: data
                        .get("parentId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    title: data
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    chunk: data
                        .get("chunk")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    context_snapshot: data
                        .get("contextSnapshot")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.clarify_trace.on_thinking_trace.as_mut() {
                    hook(info);
                }
            }
        }
        "timeline_log" => {
            if let Some(data) = val.get("data") {
                let info = TimelineLogInfo {
                    kind: data
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    title: data
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    detail: data
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                };
                if let Some(hook) = sink.notice_timeline.on_timeline_log.as_mut() {
                    hook(info);
                }
            }
        }
        "conversation_saved" => {
            if let Some(data) = val.get("data") {
                let revision = data.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);
                let tiktoken = None;
                if let Some(hook) = sink.notice_timeline.on_conversation_saved_revision.as_mut() {
                    hook(revision, tiktoken);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use crate::sse_dispatch::{
        SseClarifyTraceHooks, SseNoticeTimelineHooks, SseStagedPlanHooks, SseWorkspaceToolHooks,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    fn dummy_sink() -> SseControlSink<'static> {
        // 使用 Box::leak 创建静态闭包，避免临时引用生命周期问题
        let on_err: &'static mut dyn FnMut(String) = Box::leak(Box::new(|_| {}));
        SseControlSink {
            user_locale: Locale::ZhHans,
            on_error: on_err,
            workspace_tool: SseWorkspaceToolHooks::default(),
            staged_plan: SseStagedPlanHooks::default(),
            clarify_trace: SseClarifyTraceHooks::default(),
            notice_timeline: SseNoticeTimelineHooks::default(),
        }
    }

    #[test]
    fn run_finished_returns_stream_ended() {
        let parser = V2Parser;
        let mut sink = dummy_sink();
        let data = r#"{"type":"RUN_FINISHED","threadId":"th-1","runId":"run-1"}"#;
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::StreamEnded);
    }

    #[test]
    fn run_error_returns_stream_ended() {
        let parser = V2Parser;
        let mut sink = dummy_sink();
        let data = r#"{"type":"RUN_ERROR","error":{"message":"fail","code":"ERR"}}"#;
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::StreamEnded);
    }

    #[test]
    fn tool_call_result_is_handled() {
        let parser = V2Parser;
        let mut sink = dummy_sink();
        let data = r#"{"type":"TOOL_CALL_RESULT","toolCallId":"tc-1","content":"done"}"#;
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::Handled);
    }

    #[test]
    fn custom_tool_running_triggers_hook() {
        let parser = V2Parser;
        let called = Rc::new(RefCell::new(false));
        let called2 = Rc::clone(&called);
        let mut on_tool = |b: bool| *called2.borrow_mut() = b;
        let mut sink = SseControlSink {
            user_locale: Locale::ZhHans,
            on_error: &mut |_| {},
            workspace_tool: SseWorkspaceToolHooks {
                on_tool_status_change: Some(&mut on_tool),
                ..SseWorkspaceToolHooks::default()
            },
            staged_plan: SseStagedPlanHooks::default(),
            clarify_trace: SseClarifyTraceHooks::default(),
            notice_timeline: SseNoticeTimelineHooks::default(),
        };
        let data = r#"{"type":"CUSTOM","customType":"tool_running","data":{"running":true}}"#;
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::Handled);
        assert!(*called.borrow());
    }

    #[test]
    fn unknown_type_falls_back_to_plain() {
        let parser = V2Parser;
        let mut sink = dummy_sink();
        let data = r#"{"type":"UNKNOWN","foo":"bar"}"#;
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::Plain);
    }

    #[test]
    fn non_json_falls_back_to_plain() {
        let parser = V2Parser;
        let mut sink = dummy_sink();
        let data = "hello world";
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::Plain);
    }

    #[test]
    fn multi_line_tool_call_splits() {
        let parser = V2Parser;
        let called = Rc::new(RefCell::new(0u32));
        let called2 = Rc::clone(&called);
        let mut on_tc = |_n: String,
                         _s: String,
                         _p: Option<String>,
                         _a: Option<String>,
                         _g: Option<String>,
                         _tid: Option<String>| {
            *called2.borrow_mut() += 1;
        };
        let mut sink = SseControlSink {
            user_locale: Locale::ZhHans,
            on_error: &mut |_| {},
            workspace_tool: SseWorkspaceToolHooks {
                on_tool_call: Some(&mut on_tc),
                ..SseWorkspaceToolHooks::default()
            },
            staged_plan: SseStagedPlanHooks::default(),
            clarify_trace: SseClarifyTraceHooks::default(),
            notice_timeline: SseNoticeTimelineHooks::default(),
        };
        let data = concat!(
            r#"{"type":"TOOL_CALL_START","toolCallId":"tc-1","name":"read_file"}"#,
            "\n",
            r#"{"type":"TOOL_CALL_ARGS","toolCallId":"tc-1","args":"path=/etc/hosts"}"#,
            "\n",
            r#"{"type":"TOOL_CALL_END","toolCallId":"tc-1"}"#,
        );
        let dispatch = parser.parse(data, &mut sink);
        assert_eq!(dispatch, SseDispatch::Handled);
        // TOOL_CALL_START should trigger on_tool_call once
        assert_eq!(*called.borrow(), 1);
    }
}
