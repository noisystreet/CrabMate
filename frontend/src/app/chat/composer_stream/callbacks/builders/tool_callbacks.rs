//! 工具相关 SSE 回调工厂（`on_tool_output_chunk` / `on_tool_result` / `on_tool_call`）。

use std::rc::Rc;

use leptos::prelude::*;

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
            }
        });
        ensure_streaming_assistant_tail_last(&stream_ctx);
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
            stream_ctx
                .scratch
                .apply_stream_control_event(StreamControlEvent::ToolCallDeclared);
            let _ = (preview, full);
            // 与后端 `tool_running` 帧互补：tool_call 往往先于或并列到达，此处立即置位可避免
            // 长耗时工具（如 git_commit）期间状态栏仍误显「模型生成中」。
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
            // 工具占位气泡已有 `msg-loading` 呼吸边框；不再拼接「· 工具执行中…」，避免终态摘要重复该文案。
            let text = to_single_line(&core, 140);
            let detail = if !name.trim().is_empty() {
                format!("tool: {name}\nstatus: running")
            } else {
                "status: running".to_string()
            };
            let id = make_message_id();
            let marker = goal_id
                .as_deref()
                .map(|g| format!("hierarchical-subgoal:{g}"))
                .or_else(|| accum.current_subgoal_marker_cloned());
            let tcid = tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            stream_ctx.update_bound_session(|s| {
                let msg = StoredMessage {
                    id: id.clone(),
                    role: "system".to_string(),
                    text,
                    reasoning_text: detail.clone(),
                    image_urls: vec![],
                    state: Some(StoredMessageState::Loading),
                    is_tool: true,
                    tool_call_id: tcid.clone(),
                    tool_name: non_empty_trimmed_tool_name(&name),
                    created_at: message_created_ms(),
                };
                if let Some(mk) = marker.as_deref()
                    && let Some(idx) = s.messages.iter().rposition(|m| {
                        m.state
                            .as_ref()
                            .is_some_and(|st| st.matches_full_marker(mk))
                    })
                {
                    s.messages.insert(idx + 1, msg);
                } else {
                    s.messages.push(msg);
                }
            });
            // 开场白留在时间线/工具之上；工具后挂新占位，续写走新气泡，避免“最早的话出现在最下面”。
            finalize_loading_assistant_before_tool_and_tail_with_new_loading(&stream_ctx, &id);
            // 有 `tool_call_id` 时由 `tool_result` 按 id 命中占位气泡；否则保持 FIFO。
            if tcid.is_none() {
                stream_ctx.scratch.enqueue_pending_tool_message_id(id);
            }
        },
    )
}
