//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口，便于审阅与后续按事件拆文件。

use std::cell::Cell;
use std::rc::Rc;

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
            if in_answer_phase.get() {
                append_to_assistant_text(aid, mid, &chunk, &stream_ctx.chat.sessions);
            }
            // 不在 answer 阶段时跳过（thinking 内容直接通过 on_timeline_log/on_thinking_trace 流入 text）
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
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                        && m.state.as_deref() == Some("loading")
                    {
                        m.state = None;
                        if m.text.trim().is_empty() {
                            m.text = i18n::stream_empty_reply(loc).to_string();
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
            move |_name: String,
                  _summary: String,
                  preview: Option<String>,
                  full: Option<String>| {
                let args = super::context::PendingToolArgs { preview, full };
                *stream_ctx.pending_tool_args.borrow_mut() = args;
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

    // Manager 规划 / 分层执行内容 → 追加到同一 assistant 消息 text，流式展示
    let on_timeline_log: Rc<dyn Fn(TimelineLogInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: TimelineLogInfo| {
            web_sys::console::log_1(
                &format!("[TL] kind={} title={}", info.kind, info.title).into(),
            );
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.as_str();
            let chunk = format!("{}\n\n", info.title);
            append_to_assistant_text(aid, mid, &chunk, &stream_ctx.chat.sessions);
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

    // thinking_trace 内容 → 追加到同一 assistant 消息 text
    let on_thinking_trace: Rc<dyn Fn(crate::sse_dispatch::ThinkingTraceInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: crate::sse_dispatch::ThinkingTraceInfo| {
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.as_str();
            let title = info.title.as_deref().unwrap_or(&info.op);
            let chunk = format!("{}\n\n", title);
            append_to_assistant_text(aid, mid, &chunk, &stream_ctx.chat.sessions);
        })
    };

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
