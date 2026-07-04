//! 时间线旁注文案与子目标 upsert；**布局转移**见 [`super::super::turn_layout::TurnLayout`]。

use leptos::prelude::GetUntracked;

use crate::i18n;
use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::stream_overlay_merged_text_reasoning_owned;

use super::super::turn_layout::{TurnLayout, insert_assistant_before_loading_tail};
use crate::app::chat::composer_stream::context::ChatStreamCallbackCtx;
use crate::message_dedupe::assistant_texts_fuzzy_duplicate;

pub(crate) fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<StoredMessageState>,
) {
    if text.trim().is_empty() {
        return;
    }
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
        created_at: message_created_ms(),
    };
    TurnLayout::push_assistant_timeline(stream_ctx, msg);
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
            s.messages.iter().any(|m| {
                m.role == "assistant"
                    && !m.is_tool
                    && (m.text.trim() == needle
                        || assistant_texts_fuzzy_duplicate(m.text.as_str(), needle))
            })
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
                let visible =
                    stream_overlay_merged_text_reasoning_owned(m, overlay.as_ref(), sid.as_str())
                        .map(|(t, _)| t)
                        .unwrap_or_else(|| m.text.clone());
                visible.trim() == needle
                    || assistant_texts_fuzzy_duplicate(visible.as_str(), needle)
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
        insert_assistant_before_loading_tail(&mut s.messages, mid.as_str(), msg);
    });
    TurnLayout::pin_loading_tail(stream_ctx);
}
