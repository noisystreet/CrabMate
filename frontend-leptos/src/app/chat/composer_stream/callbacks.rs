//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口，便于审阅与后续按事件拆文件。

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use crate::api::ChatStreamCallbacks;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::message_format::{staged_timeline_system_message_body, tool_card_text};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, ThinkingTraceInfo, TimelineLogInfo, ToolResultInfo,
};
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    timeline_state_approval_decision, timeline_state_staged_end, timeline_state_staged_start,
    timeline_state_tool,
};

use super::context::ChatStreamCallbackCtx;

/// 由 [`super::make_attach_chat_stream`](super::make_attach_chat_stream) 调用；集中所有 `on_*` 闭包，降低 `mod.rs` 维护面。
pub(super) fn build_chat_stream_callbacks(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
) -> ChatStreamCallbacks {
    let on_delta: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        Rc::new(move |chunk: String| {
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.as_str();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                        if in_answer_phase.get() {
                            m.text.push_str(&chunk);
                        } else {
                            m.reasoning_text.push_str(&chunk);
                        }
                    }
                }
            });
        })
    };
    let on_done: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move || {
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.clone();
            // 先保存 reasoning_text（hydration 会覆盖 sessions）
            let reasoning_backup = stream_ctx.chat.sessions.with_untracked(|list| {
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|m| m.id == mid))
                    .map(|m| m.reasoning_text.clone())
                    .unwrap_or_default()
            });
            if !reasoning_backup.is_empty() {
                stream_ctx.chat.reasoning_preserved.update(|map| {
                    map.insert(mid.clone(), reasoning_backup);
                });
            }
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid)
                    && let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                    && m.state.as_deref() == Some("loading")
                {
                    m.state = None;
                    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
                        m.text = i18n::stream_empty_reply(loc).to_string();
                    }
                }
            });
            stream_ctx.shell.status_busy.set(false);
            *stream_ctx.shell.abort_cell.lock().unwrap() = None;
            stream_ctx
                .chat
                .session_hydrate_nonce
                .update(|n| *n = n.wrapping_add(1));
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
    let on_tool_status: Rc<dyn Fn(bool)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |b: bool| {
            stream_ctx.shell.tool_busy.set(b);
        })
    };
    let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ToolResultInfo| {
            let t = tool_card_text(&info, stream_ctx.locale.get_untracked());
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let tl_ok = info.ok.unwrap_or(true);
            let state = timeline_state_tool(&id, tl_ok);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text: t,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: true,
                        created_at: message_created_ms(),
                    });
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
        Rc::new(move |reason: String| {
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
    let on_thinking_trace: Rc<dyn Fn(ThinkingTraceInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ThinkingTraceInfo| {
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.clone();
            let trace_text = if let (Some(title), Some(chunk)) = (&info.title, &info.chunk) {
                format!("{}\n{}\n\n", title, chunk)
            } else if let Some(title) = &info.title {
                format!("{}\n\n", title)
            } else if let Some(chunk) = &info.chunk {
                format!("{}\n\n", chunk)
            } else {
                String::new()
            };
            if !trace_text.is_empty() {
                stream_ctx.chat.sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        if let Some(m) = s.messages.iter_mut().find(|msg| msg.id == mid) {
                            m.reasoning_text.push_str(&trace_text);
                        }
                    }
                });
            }
            stream_ctx.shell.thinking_trace_log.update(|v| v.push(info));
        })
    };
    let on_timeline_log: Rc<dyn Fn(TimelineLogInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: TimelineLogInfo| {
            let aid = stream_ctx.active_session_id.as_str();
            let msg_id = stream_ctx.assistant_message_id.clone();
            let state = timeline_state_approval_decision(&msg_id, &info.kind);
            let text = staged_timeline_system_message_body(&info.title);
            // 同时追加到助手消息的 reasoning_text（供思考区展示）
            let timeline_for_thinking = format!("{}\n\n", info.title);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    // 追加到助手消息的 reasoning_text
                    if let Some(m) = s.messages.iter_mut().find(|msg| msg.id == msg_id) {
                        m.reasoning_text.push_str(&timeline_for_thinking);
                    }
                    // 创建 timeline system 消息（原有行为）
                    s.messages.push(StoredMessage {
                        id: msg_id.clone(),
                        role: "system".to_string(),
                        text,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        created_at: message_created_ms(),
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

    ChatStreamCallbacks {
        on_delta,
        on_done: on_done.clone(),
        on_error: on_error.clone(),
        on_workspace_changed: on_ws,
        on_tool_status,
        on_tool_result,
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
