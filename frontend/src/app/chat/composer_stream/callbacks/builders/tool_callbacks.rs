//! 工具相关 SSE 回调工厂（`on_tool_output_chunk` / `on_tool_result` / `on_tool_call`）。

use std::rc::Rc;

use leptos::prelude::*;

use super::super::turn_layout::TurnLayout;
use crate::api::OnToolCallFn;
use crate::app::stream_shell_busy::StreamShellBusyOp;
use crate::i18n;
use crate::message_format::{strip_ansi_codes, tool_stored_text_from_result_info};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{ToolOutputChunkInfo, ToolResultInfo};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::timeline_scan::timeline_state_tool;

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;
use super::super::super::stream_control_reducer::StreamControlEvent;
use super::super::helpers::*;

pub(in super::super) fn make_on_tool_output_chunk(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(ToolOutputChunkInfo)> {
    Rc::new(move |info: ToolOutputChunkInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx
            .scratch
            .apply_stream_control_event(StreamControlEvent::ToolOutputChunk);
        let tid = info.tool_call_id.trim();
        if tid.is_empty() {
            return;
        }
        stream_ctx.update_bound_session(|s| {
            let idx_opt = index_of_tool_message_by_call_id_latest(&s.messages, tid);
            if let Some(idx) = idx_opt {
                if info.name.as_deref() == Some("terminal_session") {
                    s.messages[idx]
                        .reasoning_text
                        .push_str(&strip_ansi_codes(&info.chunk));
                } else {
                    s.messages[idx].reasoning_text.push_str(&info.chunk);
                }
            }
        });
    })
}

pub(in super::super) fn make_on_tool_result(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(ToolResultInfo)> {
    Rc::new(move |info: ToolResultInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx
            .scratch
            .apply_stream_control_event(StreamControlEvent::ToolResult);
        let loc = stream_ctx.locale.get_untracked();
        let stored = tool_stored_text_from_result_info(&info, loc);
        let t = stored.compact.clone();
        let detail = stored.detail.clone();

        let id = make_message_id();
        let tl_ok = info.ok.unwrap_or(true);
        let state = timeline_state_tool(&id, tl_ok);
        let stream_ctx_rc = Rc::clone(&stream_ctx);
        let mut updated_existing = false;
        let mut inserted_new_tool = false;
        stream_ctx.update_bound_session(|s| {
            let tid = info
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty());
            let idx_by_tid = tid.and_then(|t| index_of_loading_tool_by_call_id(&s.messages, t));
            let fifo_id = idx_by_tid
                .is_none()
                .then(|| stream_ctx_rc.scratch.take_pending_tool_fifo_head())
                .flatten();
            let idx_opt = idx_by_tid.or_else(|| {
                fifo_id
                    .as_deref()
                    .and_then(|pid| index_of_message_id(&s.messages, pid))
            });
            if let Some(idx) = idx_opt {
                let m = &mut s.messages[idx];
                m.text = t.clone();
                m.reasoning_text = detail.clone();
                m.state = Some(state.clone());
                m.is_tool = true;
                if m.tool_call_id.is_none() {
                    m.tool_call_id = info.tool_call_id.clone().filter(|x| !x.trim().is_empty());
                }
                if let Some(tn) = non_empty_trimmed_tool_name(&info.name) {
                    m.tool_name = Some(tn);
                }
                updated_existing = true;
            }
            if !updated_existing {
                let msg = StoredMessage {
                    id: id.clone(),
                    role: "system".to_string(),
                    text: t.clone(),
                    reasoning_text: detail.clone(),
                    image_urls: vec![],
                    state: Some(state.clone()),
                    is_tool: true,
                    tool_call_id: info.tool_call_id.clone().filter(|x| !x.trim().is_empty()),
                    tool_name: non_empty_trimmed_tool_name(&info.name),
                    created_at: message_created_ms(),
                };
                if let Some(goal_id) = info.goal_id.as_deref() {
                    let marker = format!("hierarchical-subgoal:{goal_id}");
                    if let Some(idx) = s.messages.iter().rposition(|m| {
                        m.state
                            .as_ref()
                            .is_some_and(|st| st.matches_full_marker(marker.as_str()))
                    }) {
                        s.messages.insert(idx + 1, msg);
                    } else {
                        s.messages.push(msg);
                    }
                } else {
                    s.messages.push(msg);
                }
                inserted_new_tool = true;
            }
        });
        if inserted_new_tool {
            TurnLayout::on_tool_result_inserted(&stream_ctx, id.as_str());
        } else {
            TurnLayout::pin_loading_tail(&stream_ctx);
        }
    })
}

pub(in super::super) fn chat_stream_on_tool_call_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> OnToolCallFn {
    Rc::new(
        move |name: String,
              summary: String,
              preview: Option<String>,
              full: Option<String>,
              goal_id: Option<String>,
              tool_call_id: Option<String>| {
            if stream_ctx.is_stale() {
                return;
            }
            TurnLayout::demote_answer_before_tools(stream_ctx.as_ref(), accum.as_ref());
            stream_ctx
                .scratch
                .apply_stream_control_event(StreamControlEvent::ToolCallDeclared);
            let _ = (preview, full);
            stream_ctx
                .shell
                .stream
                .apply_busy_op(StreamShellBusyOp::MirrorToolRunning(true));
            let loc = stream_ctx.locale.get_untracked();
            let core = if !summary.trim().is_empty() {
                summary.trim().to_string()
            } else if !name.trim().is_empty() {
                format!("{}{}", i18n::tool_card_prefix(loc), name.trim())
            } else {
                i18n::tool_card_fallback(loc).to_string()
            };
            let text = to_single_line(&core, 140);
            let detail = if !name.trim().is_empty() {
                format!("tool: {name}\nstatus: running")
            } else {
                "status: running".to_string()
            };
            let id = make_message_id();
            let subgoal_marker = goal_id
                .as_deref()
                .map(|g| format!("hierarchical-subgoal:{g}"))
                .or_else(|| accum.current_subgoal_marker_cloned());
            let tcid = tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let tool_msg = StoredMessage {
                id: id.clone(),
                role: "system".to_string(),
                text,
                reasoning_text: detail,
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: true,
                tool_call_id: tcid.clone(),
                tool_name: non_empty_trimmed_tool_name(&name),
                created_at: message_created_ms(),
            };
            TurnLayout::on_tool_call_declared(
                stream_ctx.as_ref(),
                tool_msg,
                subgoal_marker.as_deref(),
            );
            if let Some(ref tcid) = tcid {
                stream_ctx
                    .scratch
                    .on_turn_tool_call(tcid.as_str(), name.trim(), core.as_str());
                stream_ctx
                    .scratch
                    .sync_turn_projection(stream_ctx.as_ref(), true);
            }
            if tcid.is_none() {
                stream_ctx.scratch.enqueue_pending_tool_message_id(id);
            }
        },
    )
}
