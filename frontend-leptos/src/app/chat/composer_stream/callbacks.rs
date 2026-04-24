//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口，便于审阅与后续按事件拆文件。

use std::cell::Cell;
use std::rc::Rc;
use std::{cell::RefCell, collections::VecDeque};

use leptos::prelude::*;

use crate::api::ChatStreamCallbacks;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::i18n::Locale;
use crate::message_format::{staged_timeline_system_message_body, tool_card_text};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, TimelineLogInfo, ToolResultInfo,
};
use crate::storage::{ChatSession, StoredMessage};
use crate::timeline_scan::{
    timeline_state_staged_end, timeline_state_staged_start, timeline_state_tool,
};

use super::context::ChatStreamCallbackCtx;

fn enqueue_pending_tool_message_id(queue: &Rc<RefCell<VecDeque<String>>>, message_id: String) {
    queue.borrow_mut().push_back(message_id);
}

fn take_pending_tool_message_id(queue: &Rc<RefCell<VecDeque<String>>>) -> Option<String> {
    queue.borrow_mut().pop_front()
}

fn build_final_response_text(title: &str, detail: Option<&str>) -> String {
    let mut final_text = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !final_text.is_empty() {
            final_text.push_str("\n\n");
        }
        final_text.push_str(detail);
    }
    final_text
}

/// 根据暂存的工具调用参数生成参数展示文本。
fn build_tool_args_text(args: &super::context::PendingToolArgs, loc: Locale) -> String {
    let mut out = String::new();
    let args_content = args.full.as_ref().or(args.preview.as_ref());
    let Some(content) = args_content else {
        return String::new();
    };
    let label = if args.full.is_some() {
        i18n::tool_call_args_label(loc)
    } else {
        i18n::tool_call_args_preview_label(loc)
    };
    out.push_str(label);
    out.push_str("\n");

    // 对于 run_command，将 command 字段放在 args 之前显示
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if v.get("command").is_some() && v.get("args").is_some() {
            let mut reordered = serde_json::Map::new();
            if let Some(cmd) = v.get("command").cloned() {
                reordered.insert("command".to_string(), cmd);
            }
            if let Some(a) = v.get("args").cloned() {
                reordered.insert("args".to_string(), a);
            }
            // 保留其他字段
            if let Some(obj) = v.as_object() {
                for (k, v) in obj.iter() {
                    if k != "command" && k != "args" {
                        reordered.insert(k.clone(), v.clone());
                    }
                }
            }
            out.push_str(
                &serde_json::to_string(&serde_json::Value::Object(reordered))
                    .unwrap_or_else(|_| content.to_string()),
            );
            return out;
        }
    }

    out.push_str(content);
    out
}

/// 将内容追加到正在流式生成的 assistant 消息 text 中。
fn append_to_assistant_text(
    aid: &str,
    mid: &str,
    chunk: &str,
    sessions: &RwSignal<Vec<ChatSession>>,
) {
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                m.text.push_str(chunk);
            }
        }
    });
}

fn build_empty_reply_with_diagnostic(
    loc: Locale,
    answer_phase_entered: bool,
    answer_delta_chars: usize,
    stream_end_reason: Option<&str>,
) -> String {
    let base = if !answer_phase_entered {
        i18n::stream_empty_reply_no_answer_phase(loc)
    } else if answer_delta_chars == 0 {
        i18n::stream_empty_reply_no_delta(loc)
    } else {
        i18n::stream_empty_reply(loc)
    };
    format!(
        "{base}\n\n{}",
        i18n::stream_empty_reply_diag_line(
            loc,
            stream_end_reason,
            answer_phase_entered,
            answer_delta_chars
        )
    )
}

/// 由 [`super::make_attach_chat_stream`](super::make_attach_chat_stream) 调用；集中所有 `on_*` 闭包，降低 `mod.rs` 维护面。
pub(super) fn build_chat_stream_callbacks(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
) -> ChatStreamCallbacks {
    let answer_delta_chars: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let stream_end_reason: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    // 兜底缓冲：若服务端未下发 assistant_answer_phase，但确实有 delta，
    // 则在 on_done 且正文仍为空时回填，避免出现“后端有答复、前端无回复”。
    let pre_answer_buffer: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let on_delta: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let pre_answer_buffer = Rc::clone(&pre_answer_buffer);
        Rc::new(move |chunk: String| {
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.as_str();
            if in_answer_phase.get() {
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(chunk.chars().count()),
                );
                append_to_assistant_text(aid, mid, &chunk, &stream_ctx.chat.sessions);
            } else {
                pre_answer_buffer.borrow_mut().push_str(&chunk);
            }
        })
    };

    let on_done: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let stream_end_reason = Rc::clone(&stream_end_reason);
        let pre_answer_buffer = Rc::clone(&pre_answer_buffer);
        Rc::new(move || {
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                        && m.state.as_deref() == Some("loading")
                    {
                        m.state = None;
                        if m.text.trim().is_empty() {
                            let buffered = pre_answer_buffer.borrow();
                            if !buffered.trim().is_empty() {
                                m.text = buffered.clone();
                                answer_delta_chars.set(
                                    answer_delta_chars
                                        .get()
                                        .saturating_add(m.text.chars().count()),
                                );
                                web_sys::console::log_1(
                                    &"[SSE] fallback_from_pre_answer_buffer=true".into(),
                                );
                            }
                        }
                        if m.text.trim().is_empty() {
                            let end_reason = stream_end_reason.borrow();
                            m.text = build_empty_reply_with_diagnostic(
                                loc,
                                in_answer_phase.get(),
                                answer_delta_chars.get(),
                                end_reason.as_deref(),
                            );
                        }
                    }
                }
            });
            stream_ctx.shell.status_busy.set(false);
            *stream_ctx.shell.abort_cell.lock().unwrap() = None;
        })
    };

    let on_error: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |msg: String| {
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            stream_ctx.chat.stream_job_id.set(None);
            stream_ctx.chat.stream_last_event_seq.set(0);
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                        m.text = msg;
                        m.state = Some("error".to_string());
                    }
                }
            });
            stream_ctx.shell.status_busy.set(false);
            stream_ctx.shell.status_err.set(Some(
                i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
            ));
            *stream_ctx.shell.abort_cell.lock().unwrap() = None;
        })
    };

    let on_ws: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move || {
            (stream_ctx.shell.refresh_workspace)();
            if stream_ctx.shell.changelist_modal_open.get_untracked() {
                stream_ctx
                    .shell
                    .changelist_fetch_nonce
                    .update(|x| *x = x.wrapping_add(1));
            }
        })
    };

    // 暂存 tool_call 参数
    let on_tool_call: Rc<dyn Fn(String, String, Option<String>, Option<String>)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(
            move |name: String, summary: String, preview: Option<String>, full: Option<String>| {
                let args = super::context::PendingToolArgs { preview, full };
                *stream_ctx.pending_tool_args.borrow_mut() = args;
                let args_text = build_tool_args_text(
                    &stream_ctx.pending_tool_args.borrow(),
                    stream_ctx.locale.get_untracked(),
                );
                let loc = stream_ctx.locale.get_untracked();
                let mut parts: Vec<String> = Vec::new();
                if !summary.trim().is_empty() {
                    parts.push(summary.trim().to_string());
                } else if !name.trim().is_empty() {
                    parts.push(format!("{}{}", i18n::tool_card_prefix(loc), name.trim()));
                } else {
                    parts.push(i18n::tool_card_fallback(loc).to_string());
                }
                parts.push(i18n::status_tool_running(loc).to_string());
                if !args_text.is_empty() {
                    parts.push(args_text);
                }
                let text = parts.join("\n\n");
                let id = make_message_id();
                let aid = stream_ctx.active_session_id.as_str();
                stream_ctx.chat.sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        s.messages.push(StoredMessage {
                            id: id.clone(),
                            role: "system".to_string(),
                            text,
                            reasoning_text: String::new(),
                            image_urls: vec![],
                            state: Some("loading".to_string()),
                            is_tool: true,
                            created_at: message_created_ms(),
                        });
                    }
                });
                enqueue_pending_tool_message_id(&stream_ctx.pending_tool_message_ids, id);
            },
        )
    };

    let on_tool_status: Rc<dyn Fn(bool)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |b: bool| {
            stream_ctx.shell.tool_busy.set(b);
        })
    };

    let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ToolResultInfo| {
            // 获取暂存的参数并构建参数文本
            let pending_args = stream_ctx.pending_tool_args.borrow().clone();
            let args_text = build_tool_args_text(&pending_args, stream_ctx.locale.get_untracked());

            let result_text = tool_card_text(&info, stream_ctx.locale.get_untracked());
            // 结果在前，参数在后
            let t = if !args_text.is_empty() {
                format!("{}\n\n{}", result_text, args_text)
            } else {
                result_text
            };

            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let tl_ok = info.ok.unwrap_or(true);
            let state = timeline_state_tool(&id, tl_ok);
            let pending_id = take_pending_tool_message_id(&stream_ctx.pending_tool_message_ids);
            let mut updated_existing = false;
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(pid) = pending_id.as_deref()
                        && let Some(m) = s.messages.iter_mut().find(|m| m.id == pid)
                    {
                        m.text = t.clone();
                        m.state = Some(state.clone());
                        m.is_tool = true;
                        updated_existing = true;
                    }
                    if !updated_existing {
                        s.messages.push(StoredMessage {
                            id: id.clone(),
                            role: "system".to_string(),
                            text: t.clone(),
                            reasoning_text: String::new(),
                            image_urls: vec![],
                            state: Some(state.clone()),
                            is_tool: true,
                            created_at: message_created_ms(),
                        });
                    }
                }
            });
        })
    };

    let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |req: CommandApprovalRequest| {
            stream_ctx.shell.pending_approval.set(Some((
                stream_ctx.approval_session_store_id.clone(),
                req.command,
                req.args,
            )));
        })
    };

    let on_cid: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |id: String| {
            stream_ctx
                .chat
                .session_sync
                .update(|s| s.apply_stream_conversation_id(id.clone()));
            let aid = stream_ctx.active_session_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                    s.server_conversation_id = Some(id);
                    s.server_revision = None;
                }
            });
        })
    };

    let on_conv_rev: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |rev: u64| {
            stream_ctx
                .chat
                .session_sync
                .update(|s| s.apply_saved_revision(rev));
            let aid = stream_ctx.active_session_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                    s.server_revision = Some(rev);
                }
            });
        })
    };

    let on_stream_ended: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let stream_end_reason = Rc::clone(&stream_end_reason);
        Rc::new(move |reason: String| {
            *stream_end_reason.borrow_mut() = Some(reason.clone());
            if reason == "completed" || reason == "cancelled" {
                stream_ctx.chat.stream_job_id.set(None);
                stream_ctx.chat.stream_last_event_seq.set(0);
            }
        })
    };

    let on_stream_job_id: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |jid: u64| {
            stream_ctx.chat.stream_job_id.set(Some(jid));
        })
    };

    let on_last_sse_event_id: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |seq: u64| {
            stream_ctx.chat.stream_last_event_seq.set(seq);
        })
    };

    let on_assistant_answer_phase: Rc<dyn Fn()> = {
        let in_answer_phase = Rc::clone(&in_answer_phase);
        Rc::new(move || in_answer_phase.set(true))
    };

    let on_staged_step_started: Rc<dyn Fn(StagedPlanStepStartInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: StagedPlanStepStartInfo| {
            let loc = stream_ctx.locale.get_untracked();
            let text = staged_timeline_system_message_body(&i18n::timeline_staged_step_started(
                loc,
                info.step_index,
                info.total_steps,
                &info.description,
                info.executor_kind.as_deref(),
            ));
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let now = message_created_ms();
            let state = timeline_state_staged_start(&id, info.step_index, info.total_steps);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        created_at: now,
                    });
                }
            });
        })
    };

    let on_clarification: Rc<dyn Fn(ClarificationQuestionnaireInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ClarificationQuestionnaireInfo| {
            stream_ctx
                .shell
                .pending_clarification
                .set(Some(PendingClarificationForm::from_sse(info)));
        })
    };

    // Manager 规划 / 分层执行旁注：作为 system 时间线消息落盘，按时间顺序与工具卡片交替展示。
    let on_timeline_log: Rc<dyn Fn(TimelineLogInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        Rc::new(move |info: TimelineLogInfo| {
            web_sys::console::log_1(
                &format!("[TL] kind={} title={}", info.kind, info.title).into(),
            );
            if info.kind == "final_response" {
                let aid = stream_ctx.active_session_id.as_str();
                let mid = stream_ctx.assistant_message_id.as_str();
                let final_text = build_final_response_text(&info.title, info.detail.as_deref());
                if !final_text.is_empty() {
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid)
                            && let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                        {
                            m.text = final_text.clone();
                        }
                    });
                    answer_delta_chars.set(
                        answer_delta_chars
                            .get()
                            .saturating_add(final_text.chars().count()),
                    );
                }
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let normalized_title = match info.kind.as_str() {
                "tool_step_started" => {
                    i18n::timeline_tool_step_started_title(loc, info.title.trim())
                }
                "tool_step_finished" => {
                    i18n::timeline_tool_step_finished_title(loc, info.title.trim())
                }
                _ => info.title.trim().to_string(),
            };
            let mut body = normalized_title;
            if let Some(detail) = info.detail.as_deref().map(str::trim)
                && !detail.is_empty()
            {
                body.push('\n');
                body.push_str(detail);
            }
            if body.is_empty() {
                return;
            }
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let now = message_created_ms();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text: staged_timeline_system_message_body(&body),
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: None,
                        is_tool: false,
                        created_at: now,
                    });
                }
            });
        })
    };

    let on_staged_step_finished: Rc<dyn Fn(StagedPlanStepEndInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: StagedPlanStepEndInfo| {
            let loc = stream_ctx.locale.get_untracked();
            let text = staged_timeline_system_message_body(&i18n::timeline_staged_step_finished(
                loc,
                info.step_index,
                info.total_steps,
                &info.status,
                info.executor_kind.as_deref(),
            ));
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let now = message_created_ms();
            let state =
                timeline_state_staged_end(&id, info.step_index, info.total_steps, &info.status);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        created_at: now,
                    });
                }
            });
        })
    };

    // thinking_trace 保留在调试台，不再写入聊天正文，避免干扰时间线可读性。
    let on_thinking_trace: Rc<dyn Fn(crate::sse_dispatch::ThinkingTraceInfo)> =
        { Rc::new(move |_info: crate::sse_dispatch::ThinkingTraceInfo| {}) };

    ChatStreamCallbacks {
        on_delta,
        on_done: on_done.clone(),
        on_error: on_error.clone(),
        on_workspace_changed: on_ws,
        on_tool_status,
        on_tool_result,
        on_tool_call,
        on_approval,
        on_conversation_id: on_cid,
        on_conversation_revision: on_conv_rev,
        on_stream_ended,
        on_stream_job_id,
        on_last_sse_event_id,
        on_assistant_answer_phase,
        on_staged_plan_step_started: on_staged_step_started,
        on_staged_plan_step_finished: on_staged_step_finished,
        on_clarification_questionnaire: on_clarification,
        on_thinking_trace,
        on_timeline_log,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_final_response_text, enqueue_pending_tool_message_id, take_pending_tool_message_id,
    };
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    #[test]
    fn pending_tool_message_queue_is_fifo() {
        let q = Rc::new(RefCell::new(VecDeque::new()));
        enqueue_pending_tool_message_id(&q, "m1".to_string());
        enqueue_pending_tool_message_id(&q, "m2".to_string());
        enqueue_pending_tool_message_id(&q, "m3".to_string());

        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m1"));
        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m2"));
        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m3"));
        assert_eq!(take_pending_tool_message_id(&q), None);
    }

    #[test]
    fn pending_tool_message_queue_empty_returns_none() {
        let q = Rc::new(RefCell::new(VecDeque::new()));
        assert_eq!(take_pending_tool_message_id(&q), None);
    }

    #[test]
    fn final_response_text_merges_title_and_detail() {
        let merged = build_final_response_text("  你好  ", Some("  世界  "));
        assert_eq!(merged, "你好\n\n世界");
    }

    #[test]
    fn final_response_text_ignores_empty_detail() {
        let merged = build_final_response_text("  你好  ", Some("   "));
        assert_eq!(merged, "你好");
    }
}
