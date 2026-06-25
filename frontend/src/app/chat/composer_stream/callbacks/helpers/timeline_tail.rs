//! 时间线插入、流式尾泡轮换与助手行收尾。

use std::cell::RefCell;

use leptos::prelude::GetUntracked;

use crate::i18n;
use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    stream_overlay_merged_text_reasoning_owned, stream_overlay_take_into_stored_message,
};

use crate::app::chat::composer_stream::context::ChatStreamCallbackCtx;

/// 将旁注插在**当前流式 `loading` 助手气泡之前**；若无占位则追加到末尾。
pub(crate) fn insert_msg_before_streaming_assistant_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
    msg: StoredMessage,
) {
    if let Some(idx) = messages.iter().position(|m| {
        m.id == streaming_assistant_id
            && m.role == "assistant"
            && m.state.as_ref().is_some_and(|s| s.is_loading())
    }) {
        messages.insert(idx, msg);
    } else {
        messages.push(msg);
    }
}

/// 管理器时间线（意图分析、规划摘要等）在服务端往往早于正文 `delta`，
/// 须插在**当前流式 `loading` 助手气泡之前**，否则会跑到已流出的计划文字下面。
pub(crate) fn insert_before_streaming_assistant_or_append(
    stream_ctx: &ChatStreamCallbackCtx,
    msg: StoredMessage,
) {
    let mid = stream_ctx.scratch.clone_assistant_id();
    stream_ctx.update_bound_session(|s| {
        insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
    });
}

pub(crate) fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<StoredMessageState>,
) {
    if text.trim().is_empty() {
        return;
    }
    let now = message_created_ms();
    let msg = StoredMessage {
        id: make_message_id(),
        role: "assistant".to_string(),
        text,
        reasoning_text: String::new(),
        image_urls: vec![],
        state,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: now,
    };
    insert_before_streaming_assistant_or_append(stream_ctx, msg);
    ensure_streaming_assistant_tail_last(stream_ctx);
}

pub(crate) fn assistant_message_has_visible_text(
    stream_ctx: &ChatStreamCallbackCtx,
    text: &str,
) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    stream_ctx
        .read_bound_session(|s| {
            s.messages
                .iter()
                .any(|m| m.role == "assistant" && !m.is_tool && m.text.trim() == needle)
        })
        .unwrap_or(false)
}

pub(crate) fn streaming_assistant_tail_has_text(
    stream_ctx: &ChatStreamCallbackCtx,
    text: &str,
) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let overlay = stream_ctx.chat.stream_text_overlay.get_untracked();
    stream_ctx
        .read_bound_session(|s| {
            s.messages.iter().any(|m| {
                if m.id != mid || m.role != "assistant" || m.is_tool {
                    return false;
                }
                if !m.state.as_ref().is_some_and(|st| st.is_loading()) {
                    return false;
                }
                let visible =
                    stream_overlay_merged_text_reasoning_owned(m, overlay.as_ref(), sid.as_str())
                        .map(|(t, _)| t)
                        .unwrap_or_else(|| m.text.clone());
                visible.trim() == needle
            })
        })
        .unwrap_or(false)
}

pub(crate) fn extract_subgoal_marker_from_title(title: &str) -> Option<String> {
    let title = title.trim();
    for prefix in i18n::hierarchical_subgoal_title_prefixes() {
        if !title.starts_with(prefix) {
            continue;
        }
        let rest = title.strip_prefix(prefix)?;
        let goal_id = rest.strip_suffix('`')?;
        if goal_id.is_empty() {
            return None;
        }
        return Some(format!("hierarchical-subgoal:{goal_id}"));
    }
    None
}

pub(crate) fn extract_subgoal_target_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| i18n::hierarchical_goal_target_raw(line).map(|_| line.to_string()))
}

pub(crate) fn merge_subgoal_text_preserving_target(existing: &str, incoming: &str) -> String {
    if extract_subgoal_target_line(incoming).is_some() {
        return incoming.to_string();
    }
    let Some(target_line) = extract_subgoal_target_line(existing) else {
        return incoming.to_string();
    };
    let mut lines = incoming.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return format!("{target_line}\n\n");
    }
    let first_trim = lines[0].trim_start();
    let insert_idx = if i18n::hierarchical_subgoal_title_second_line_prefixes()
        .iter()
        .any(|p| first_trim.starts_with(p))
    {
        1
    } else {
        0
    };
    lines.insert(insert_idx, target_line.as_str());
    let mut out = lines.join("\n");
    if !out.ends_with("\n\n") {
        out.push_str("\n\n");
    }
    out
}

pub(crate) fn upsert_hierarchical_subgoal_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    title: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    let marker = extract_subgoal_marker_from_title(title);
    if marker.is_none() {
        push_assistant_timeline_bubble(stream_ctx, text, None);
        return;
    }
    let marker = marker.unwrap_or_default();
    let now = message_created_ms();
    stream_ctx.update_bound_session(|s| {
        if let Some(existing) = s.messages.iter_mut().find(|m| {
            m.role == "assistant"
                && m.state
                    .as_ref()
                    .is_some_and(|st| st.matches_full_marker(marker.as_str()))
        }) {
            existing.text = merge_subgoal_text_preserving_target(&existing.text, &text);
            existing.created_at = now;
            return;
        }
        let msg = StoredMessage {
            id: make_message_id(),
            role: "assistant".to_string(),
            text: text.clone(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::HierarchicalSubgoal(marker.clone())),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        };
        let mid = stream_ctx.scratch.clone_assistant_id();
        insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
    });
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 结束当前 `assistant_message_id` 指向的流式 `loading` 助手行：空正文则删除，否则去掉 `loading` state。
///
/// 供工具前收尾与无工具多轮轮换共用，避免两处复制分叉。
///
/// **注意**：`reasoning_text` 非空时视为有内容，保留气泡并清除 loading state，
/// 避免 `assistant_answer_phase` 之前的思维链在工具调用时被误删。
pub(crate) fn finalize_current_loading_streaming_assistant_row(stream_ctx: &ChatStreamCallbackCtx) {
    let sid = stream_ctx.bound_stream_session_id.clone();
    stream_ctx.update_bound_session(|s| {
        let mid_owned = stream_ctx.scratch.clone_assistant_id();
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid_owned.as_str()) {
            stream_overlay_take_into_stored_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid_owned.as_str(),
                &mut s.messages[idx],
            );
        }
        let mid = stream_ctx.scratch.borrow_assistant_id();
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            let m = &mut s.messages[idx];
            if m.role == "assistant" && m.state.as_ref().is_some_and(|st| st.is_loading()) {
                if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
                    s.messages.remove(idx);
                } else {
                    m.state = None;
                }
            }
        }
    });
}

/// 工具卡片插入前：结束当前流式段（开场白等保留在工具与时间线**之上**），
/// 并在本条工具消息之后挂新的 `loading` 占位，供工具结束后的续写。
pub(crate) fn finalize_loading_assistant_before_tool_and_tail_with_new_loading(
    stream_ctx: &ChatStreamCallbackCtx,
    tool_message_id: &str,
) {
    let tool_present = stream_ctx
        .read_bound_session(|s| s.messages.iter().any(|m| m.id == tool_message_id))
        .unwrap_or(false);
    if !tool_present {
        return;
    }
    finalize_current_loading_streaming_assistant_row(stream_ctx);
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    stream_ctx.update_bound_session(|s| {
        let Some(tidx) = s.messages.iter().position(|m| m.id == tool_message_id) else {
            return;
        };
        let new_asst_id = make_message_id();
        s.messages.insert(
            tidx + 1,
            StoredMessage {
                id: new_asst_id.clone(),
                role: "assistant".to_string(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: now,
            },
        );
        *new_tail_id.borrow_mut() = Some(new_asst_id);
    });
    if let Some(id) = new_tail_id.into_inner() {
        stream_ctx
            .scratch
            .adopt_new_assistant_tail_after_rotation(id);
    }
}

/// 同一轮 `run_agent_turn` 内可能多次调用模型（如外层 `continue 'outer` 规划改写），每次首段正文前都会再发
/// `assistant_answer_phase`。若仍写入同一 `assistant_message_id`，多段可见输出会挤在一个气泡里「不断刷新」。
/// 工具轮之间已有 [`finalize_loading_assistant_before_tool_and_tail_with_new_loading`]；此处补齐**无工具**的多轮。
pub(crate) fn rotate_streaming_assistant_for_followup_model_round(
    stream_ctx: &ChatStreamCallbackCtx,
) {
    finalize_current_loading_streaming_assistant_row(stream_ctx);
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    stream_ctx.update_bound_session(|s| {
        let new_asst_id = make_message_id();
        s.messages.push(StoredMessage {
            id: new_asst_id.clone(),
            role: "assistant".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
        *new_tail_id.borrow_mut() = Some(new_asst_id);
    });
    if let Some(id) = new_tail_id.into_inner() {
        stream_ctx
            .scratch
            .adopt_new_assistant_tail_after_rotation(id);
    }
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 工具后续写段：分步/时间线等仍会 `push` 到列表末尾，需把当前 `loading` 占位再次移到最下方。
pub(crate) fn ensure_streaming_assistant_tail_last(stream_ctx: &ChatStreamCallbackCtx) {
    if !stream_ctx.scratch.post_tool_stream_tail_active() {
        return;
    }
    let mid = stream_ctx.scratch.clone_assistant_id();
    stream_ctx.update_bound_session(|s| {
        let Some(idx) = s.messages.iter().position(|m| m.id == mid) else {
            return;
        };
        if s.messages[idx].role != "assistant"
            || !s.messages[idx]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
        {
            return;
        }
        let m = s.messages.remove(idx);
        s.messages.push(m);
    });
}

pub(crate) fn remove_loading_assistant_placeholder(stream_ctx: &ChatStreamCallbackCtx) {
    let sid = stream_ctx.bound_stream_session_id.clone();
    let mid_owned = stream_ctx.scratch.clone_assistant_id();
    stream_ctx.update_bound_session(|s| {
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid_owned.as_str())
            && s.messages[idx].role == "assistant"
            && s.messages[idx]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
        {
            stream_overlay_take_into_stored_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid_owned.as_str(),
                &mut s.messages[idx],
            );
            s.messages.remove(idx);
        }
    });
}
