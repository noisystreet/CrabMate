//! 单条聊天消息行的根视图 [`chat_message_row`]。

use leptos::prelude::*;

use crate::message_format::{
    is_staged_timeline_bubble, message_text_for_display_ex, stored_message_is_staged_planner_round,
};
use crate::session_ops::{format_msg_time_label, write_clipboard_text};
use crate::session_search::normalize_search_query;

use super::super::message_row_actions::MessageRowActionSignals;
use super::super::message_row_user_layout::{
    chat_row_wrap_and_user_styles, tool_bubble_detail_and_jump_uid,
};
use super::ChatMessageRowSignals;
use super::helpers::{
    build_subgoal_exec_banner_icon_key, build_subgoal_exec_banner_text,
    extract_hierarchical_goal_target, extract_hierarchical_metrics,
    extract_hierarchical_phase_chip, hierarchical_subgoal_banner_is_active,
    message_row_loading_and_error, message_row_prefixed_class, message_row_shell_class,
};
use super::views::{
    ChatMessageRowBodyCoreParams, MessageActionsBarParams, build_message_actions_bar,
    chat_message_row_body_core, chat_message_row_meta_view,
    chat_message_row_subgoal_exec_banner_view,
};

pub(crate) fn chat_message_row(s: ChatMessageRowSignals) -> impl IntoView {
    let ChatMessageRowSignals {
        msg_idx,
        m,
        chat,
        collapsed_long_assistant_ids,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        auto_scroll_chat,
        status_busy,
        regen_stream_after_truncate,
        retry_assistant_target,
        status_err,
        locale,
        markdown_render,
        apply_assistant_display_filters,
    } = s;
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let row_actions = MessageRowActionSignals {
        chat,
        regen_stream_after_truncate,
        status_err,
        locale,
    };
    let is_staged_timeline = is_staged_timeline_bubble(&m);
    let cls = message_row_shell_class(is_staged_timeline, &m);
    let (loading, err) =
        message_row_loading_and_error(m.is_tool, m.role.as_str(), m.state.as_ref());
    let class_prefix = message_row_prefixed_class(cls, err, loading);
    let mid_highlight = m.id.clone();
    let time_str = format_msg_time_label(m.created_at).unwrap_or_default();
    let mid_retry = m.id.clone();
    let copy_id = m.id.clone();
    let user_retry_id = m.id.clone();
    let user_branch_id = m.id.clone();
    let is_user_plain = m.role == "user" && !m.is_tool;
    let is_tool_bubble = m.is_tool;
    let (wrap_class, user_row_outer_style, user_row_stack_style, user_row_bubble_style) =
        chat_row_wrap_and_user_styles(m.role.as_str(), is_tool_bubble);
    let tool_detail_open = RwSignal::new(false);
    let (tool_detail_text, jump_uid) = tool_bubble_detail_and_jump_uid(
        is_tool_bubble,
        m.reasoning_text.clone(),
        sessions,
        active_id,
        msg_idx,
    );
    let show_msg_action_bar = !is_tool_bubble || is_user_plain || err;
    let show_planner_round_badge = stored_message_is_staged_planner_round(&m);
    let loc_ut = locale.get_untracked();
    let subgoal_phase_chip = extract_hierarchical_phase_chip(&m, loc_ut);
    let subgoal_metrics_line = extract_hierarchical_metrics(&m, loc_ut);
    let subgoal_target_line = extract_hierarchical_goal_target(&m);
    let phase_for_banner = subgoal_phase_chip.as_ref().map(|(phase, _)| phase.as_str());
    let subgoal_exec_banner =
        build_subgoal_exec_banner_text(loc_ut, phase_for_banner, subgoal_target_line.as_deref());
    let subgoal_exec_banner_icon_key = build_subgoal_exec_banner_icon_key(loc_ut, phase_for_banner);
    let is_active_subgoal_banner = sessions.with(|list| {
        hierarchical_subgoal_banner_is_active(
            list,
            active_id.get_untracked().as_str(),
            m.id.as_str(),
            subgoal_exec_banner.as_ref(),
            phase_for_banner,
            loc_ut,
        )
    });
    let msg_core = chat_message_row_body_core(ChatMessageRowBodyCoreParams {
        m: m.clone(),
        sessions,
        active_id,
        collapsed_long_assistant_ids,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        chat_find_query,
        is_tool_bubble,
        tool_detail_text: tool_detail_text.clone(),
        tool_detail_open,
        jump_uid,
        auto_scroll_chat,
    });
    let mid_dom = m.id.clone();
    let detail_for_drawer_when = tool_detail_text.clone();
    let detail_for_drawer_text = tool_detail_text.clone();
    view! {
        <div class=wrap_class style=user_row_outer_style>
            <div class="msg-stack" style=user_row_stack_style>
                {true.then(|| {
                    chat_message_row_meta_view(
                        locale,
                        show_planner_round_badge,
                        is_staged_timeline,
                        m.clone(),
                        time_str.clone(),
                    )
                })}
                <div
                    class=move || {
                        let mut c = class_prefix.clone();
                        if !is_tool_bubble {
                            c.push_str(" msg-has-inline-copy");
                        }
                        let q = normalize_search_query(&chat_find_query.get());
                        if !q.is_empty() {
                            let in_list = chat_find_match_ids.with(|ids| {
                                ids.iter().any(|x| x == &mid_highlight)
                            });
                            if in_list {
                                c.push_str(" msg-find-match");
                            }
                            let cur = chat_find_cursor.get();
                            let is_current = chat_find_match_ids.with(|ids| {
                                ids
                                    .get(cur)
                                    .map(|x| x == &mid_highlight)
                                    .unwrap_or(false)
                            });
                            if is_current {
                                c.push_str(" msg-find-highlight");
                            }
                        }
                        c
                    }
                    id=format!("msg-{mid_dom}")
                    data-testid=move || {
                        if is_tool_bubble {
                            "chat-tool-card"
                        } else {
                            "chat-message-row"
                        }
                    }
                    style=user_row_bubble_style
                >
                    {(!is_tool_bubble).then(|| {
                        view! {
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-copy-inside-btn"
                                prop:title=move || crate::i18n::msg_copy_title(locale.get())
                                prop:aria-label=move || crate::i18n::msg_copy_aria(locale.get())
                                on:click=move |_| {
                                    let loc = locale.get_untracked();
                                    let apply = apply_assistant_display_filters.get_untracked();
                                    let t = sessions.with(|list| {
                                        let aid = active_id.get_untracked();
                                        list.iter()
                                            .find(|s| s.id == aid)
                                            .and_then(|s| {
                                                s.messages
                                                    .iter()
                                                    .find(|msg| msg.id == copy_id)
                                            })
                                            .map(|msg| {
                                                message_text_for_display_ex(msg, loc, apply)
                                            })
                                            .unwrap_or_default()
                                    });
                                    write_clipboard_text(&t, loc);
                                }
                            >
                                <svg
                                    class="msg-action-icon"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    xmlns="http://www.w3.org/2000/svg"
                                    aria-hidden="true"
                                >
                                    <rect
                                        x="9"
                                        y="9"
                                        width="13"
                                        height="13"
                                        rx="2"
                                        stroke="currentColor"
                                        stroke-width="2"
                                    />
                                    <path
                                        d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"
                                        stroke="currentColor"
                                        stroke-width="2"
                                    />
                                </svg>
                            </button>
                        }
                    })}
                    <Show when=move || is_staged_timeline>
                        {move || {
                            let banner = crate::i18n::msg_staged_timeline_exec_banner(locale.get());
                            view! {
                                <div class="msg-subgoal-exec-banner phase-run">
                                    <span class="msg-subgoal-exec-banner-icon" aria-hidden="true">
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2.2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                        >
                                            <circle cx="12" cy="12" r="9"></circle>
                                            <path d="M12 7v5l3 2"></path>
                                        </svg>
                                    </span>
                                    <span class="msg-subgoal-exec-banner-text">{banner}</span>
                                </div>
                            }
                        }}
                    </Show>
                    {chat_message_row_subgoal_exec_banner_view(
                        subgoal_exec_banner.clone(),
                        subgoal_exec_banner_icon_key,
                        is_active_subgoal_banner,
                    )}
                    {subgoal_metrics_line.as_ref().map(|line| {
                        let line = line.clone();
                        view! {
                            <div class="msg-subgoal-metrics-line">{line}</div>
                        }
                    })}
                    {msg_core}
                    {loading.then(|| {
                        view! {
                            <span class="typing-dots" aria-hidden="true">
                                <span></span>
                                <span></span>
                                <span></span>
                            </span>
                        }
                    })}
                </div>
                <Show
                    when=move || {
                        is_tool_bubble
                            && tool_detail_open.get()
                            && detail_for_drawer_when
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty())
                    }
                >
                    <div class="msg-tool-drawer msg-tool-drawer-below-card">
                        <pre class="msg-tool-drawer-pre">
                            {detail_for_drawer_text.clone().unwrap_or_default()}
                        </pre>
                    </div>
                </Show>
                {build_message_actions_bar(MessageActionsBarParams {
                    show_msg_action_bar,
                    is_user_plain,
                    err,
                    msg_idx,
                    user_retry_id: user_retry_id.clone(),
                    user_branch_id: user_branch_id.clone(),
                    mid_retry: mid_retry.clone(),
                    row_actions,
                    retry_assistant_target,
                    status_busy,
                    locale,
                })}
            </div>
        </div>
    }
}
