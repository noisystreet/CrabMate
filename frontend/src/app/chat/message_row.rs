//! 单条消息气泡与下方操作条（复制 / 重试 / 分支等）。
//!
//! 与 `POST /chat/branch`、本地截断再生相关的副作用见 [`super::message_row_actions`]。

use leptos::prelude::*;

use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::message_format::{
    is_staged_timeline_bubble, message_text_for_display_ex, stored_message_is_staged_planner_round,
};
use crate::session_ops::{format_msg_time_label, message_role_label, write_clipboard_text};
use crate::session_search::{normalize_search_query, split_for_find_highlight};
use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

use super::message_row_actions::{MessageRowActionSignals, spawn_scroll_to_linked_user_message};
use super::message_row_user_layout::{
    chat_row_wrap_and_user_styles, tool_bubble_detail_and_jump_uid,
};

/// 聊天消息行视图所需信号与数据（缩短 [`chat_message_row`] 形参列表；勿命名为 `*Props`，与 Leptos 组件宏生成类型冲突）。
#[derive(Clone)]
pub(crate) struct ChatMessageRowSignals {
    pub msg_idx: usize,
    pub m: StoredMessage,
    pub chat: ChatSessionSignals,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub status_busy: RwSignal<bool>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub status_err: RwSignal<Option<String>>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

fn is_hierarchical_subgoal_state(state: Option<&StoredMessageState>) -> bool {
    state.is_some_and(|s| s.looks_like_hierarchical_subgoal())
}

fn tool_bubble_emoji(m: &StoredMessage) -> &'static str {
    let name = m
        .tool_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            m.reasoning_text.lines().next().and_then(|line| {
                line.trim()
                    .strip_prefix("tool:")
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
        });
    name.map(i18n::tool_kind_emoji).unwrap_or("🔧")
}

fn extract_hierarchical_phase_chip(msg: &StoredMessage, loc: Locale) -> Option<(String, String)> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    i18n::hierarchical_phase_chip_view(loc, msg.text.as_str())
}

fn extract_hierarchical_metrics(msg: &StoredMessage, loc: Locale) -> Option<String> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    let mut error_count: Option<String> = None;
    let mut stagnant_rounds: Option<String> = None;
    for line in msg.text.lines().map(str::trim) {
        if error_count.is_none()
            && let Some(v) = i18n::hierarchical_error_count_raw(line)
        {
            let v = v.trim();
            if !v.is_empty() {
                error_count = Some(v.to_string());
            }
        }
        if stagnant_rounds.is_none()
            && let Some(v) = i18n::hierarchical_stagnant_rounds_raw(line)
        {
            let v = v.trim();
            if !v.is_empty() {
                stagnant_rounds = Some(v.to_string());
            }
        }
    }
    i18n::hierarchical_metrics_line(loc, error_count.as_deref(), stagnant_rounds.as_deref())
}

fn extract_hierarchical_goal_target(msg: &StoredMessage) -> Option<String> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    msg.text.lines().map(str::trim).find_map(|line| {
        i18n::hierarchical_goal_target_raw(line)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn build_subgoal_exec_banner_text(
    loc: Locale,
    phase: Option<&str>,
    target: Option<&str>,
) -> Option<String> {
    let key = i18n::hierarchical_subgoal_phase_key(phase)?;
    let verb = i18n::hierarchical_subgoal_exec_verb(loc, key);
    if verb.is_empty() {
        return None;
    }
    let suffix = i18n::hierarchical_subgoal_running_suffix(loc);
    match (loc, target.filter(|t| !t.trim().is_empty())) {
        (Locale::ZhHans, Some(t)) => Some(format!("{verb}：{}…", t.trim())),
        (Locale::ZhHans, None) => Some(format!("{verb}{suffix}")),
        (Locale::En, Some(t)) => Some(format!("{verb}: {}…", t.trim())),
        (Locale::En, None) => Some(format!("{verb} {suffix}")),
    }
}

fn build_subgoal_exec_banner_icon_key(_loc: Locale, phase: Option<&str>) -> Option<&'static str> {
    i18n::hierarchical_subgoal_phase_key(phase)
}

fn is_running_subgoal_phase(loc: Locale, phase: Option<&str>) -> bool {
    let _ = loc;
    i18n::hierarchical_subgoal_phase_key(phase).is_some()
}

fn message_row_shell_class(is_staged_timeline: bool, m: &StoredMessage) -> &'static str {
    if is_staged_timeline {
        "msg msg-staged-timeline"
    } else {
        match m.role.as_str() {
            "user" => "msg msg-user",
            "assistant" if m.is_tool => "msg msg-tool",
            "assistant" => "msg msg-assistant",
            _ if m.is_tool => "msg msg-tool",
            _ => "msg msg-system",
        }
    }
}

fn message_row_loading_and_error(
    is_tool: bool,
    role: &str,
    state: Option<&StoredMessageState>,
) -> (bool, bool) {
    let loading = state.is_some_and(|s| s.is_loading()) && (role == "assistant" || is_tool);
    let err = state.is_some_and(|s| s.is_error());
    (loading, err)
}

fn message_row_prefixed_class(cls: &str, err: bool, loading: bool) -> String {
    if err {
        format!("{cls} msg-error")
    } else if loading {
        format!("{cls} msg-loading")
    } else {
        cls.to_string()
    }
}

fn hierarchical_subgoal_banner_is_active(
    sessions: &[ChatSession],
    active_session_id: &str,
    current_msg_id: &str,
    subgoal_exec_banner: Option<&String>,
    phase_for_run_check: Option<&str>,
    loc: Locale,
) -> bool {
    if subgoal_exec_banner.is_none() || !is_running_subgoal_phase(loc, phase_for_run_check) {
        return false;
    }
    sessions
        .iter()
        .find(|s| s.id == active_session_id)
        .and_then(|sess| {
            sess.messages
                .iter()
                .rev()
                .find(|msg| is_hierarchical_subgoal_state(msg.state.as_ref()))
        })
        .map(|msg| msg.id == current_msg_id)
        .unwrap_or(false)
}

fn chat_message_row_meta_view(
    locale: RwSignal<Locale>,
    show_planner_round_badge: bool,
    is_staged_timeline: bool,
    m_role: StoredMessage,
    time_str: String,
) -> impl IntoView {
    let role_lbl = move || {
        if is_staged_timeline {
            i18n::msg_staged_timeline_role_meta(locale.get())
        } else {
            message_role_label(&m_role, locale.get())
        }
    };
    view! {
        <div class="msg-meta" aria-hidden="true">
            <span class="msg-meta-primary">
                <span class="msg-meta-role">{role_lbl}</span>
                <Show when=move || show_planner_round_badge>
                    <span
                        class="msg-planner-round-badge"
                        prop:title=move || {
                            i18n::msg_planner_round_badge_title(locale.get())
                        }
                    >
                        {move || i18n::msg_planner_round_badge(locale.get())}
                    </span>
                </Show>
            </span>
            <span class="msg-meta-time">{time_str.clone()}</span>
        </div>
    }
}

struct ChatMessageRowBodyCoreParams {
    m: StoredMessage,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
    chat_find_query: RwSignal<String>,
    is_tool_bubble: bool,
    tool_detail_text: Option<String>,
    tool_detail_open: RwSignal<bool>,
    jump_uid: Option<String>,
    auto_scroll_chat: RwSignal<bool>,
}

fn chat_message_row_body_core(p: ChatMessageRowBodyCoreParams) -> AnyView {
    let ChatMessageRowBodyCoreParams {
        m,
        sessions,
        active_id,
        collapsed_long_assistant_ids,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        chat_find_query,
        is_tool_bubble,
        tool_detail_text,
        tool_detail_open,
        jump_uid,
        auto_scroll_chat,
    } = p;
    if m.role == "assistant" && !m.is_tool {
        return assistant_markdown_collapsible_view(
            sessions,
            active_id,
            m.id.clone(),
            collapsed_long_assistant_ids,
            locale,
            markdown_render,
            apply_assistant_display_filters,
        )
        .into_any();
    }
    let body_inner = build_non_assistant_message_body(NonAssistantMessageBodyParams {
        m_for_body: m.clone(),
        is_tool_bubble,
        tool_detail_text: tool_detail_text.clone(),
        tool_detail_open,
        locale,
        chat_find_query,
        apply_assistant_display_filters,
        jump_uid,
        auto_scroll_chat,
    });
    if m.role == "user" && !m.is_tool && !m.image_urls.is_empty() {
        let imgs: Vec<String> = m.image_urls.clone();
        view! {
            <div class="msg-user-with-images">
                <div class="msg-user-images">
                    {imgs
                        .into_iter()
                        .map(|u| {
                            view! { <img class="msg-user-img" src=u alt="" /> }.into_any()
                        })
                        .collect_view()}
                </div>
                {body_inner}
            </div>
        }
        .into_any()
    } else {
        body_inner
    }
}

fn chat_message_row_subgoal_exec_banner_view(
    subgoal_exec_banner: Option<String>,
    subgoal_exec_banner_icon_key: Option<&str>,
    is_active_subgoal_banner: bool,
) -> impl IntoView {
    subgoal_exec_banner
        .map(|banner| {
            let icon_key = subgoal_exec_banner_icon_key.unwrap_or("run").to_string();
            let active_cls = if is_active_subgoal_banner {
                " is-active-subgoal-banner"
            } else {
                ""
            };
            let banner_class = format!("msg-subgoal-exec-banner phase-{icon_key}{active_cls}");
            view! {
                <div class=banner_class>
                    <span class="msg-subgoal-exec-banner-icon" aria-hidden="true">
                        {subgoal_exec_banner_icon_view(icon_key.as_str())}
                    </span>
                    <span class="msg-subgoal-exec-banner-text" prop:title=banner.clone()>{banner.clone()}</span>
                </div>
            }
            .into_any()
        })
        .unwrap_or_else(|| ().into_any())
}

fn render_highlighted_message_text(
    msg: &StoredMessage,
    loc: Locale,
    apply_filters: bool,
    query: &str,
) -> AnyView {
    let disp = message_text_for_display_ex(msg, loc, apply_filters);
    let segs = split_for_find_highlight(&disp, query);
    segs.into_iter()
        .map(|(s, hl)| {
            if hl {
                view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
            } else {
                view! { {s} }.into_any()
            }
        })
        .collect_view()
        .into_any()
}

fn subgoal_exec_banner_icon_view(icon_key: &str) -> AnyView {
    match icon_key {
        "diagnose" => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="11" cy="11" r="7"></circle>
                <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
            </svg>
        }
        .into_any(),
        "fix" => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M14.7 6.3a4 4 0 0 0-5.6 5.6L3 18v3h3l6.1-6.1a4 4 0 0 0 5.6-5.6l-2.4 2.4-3.2-3.2 2.6-2.2z"></path>
            </svg>
        }
        .into_any(),
        "verify" => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M20 6 9 17l-5-5"></path>
            </svg>
        }
        .into_any(),
        "escalate" => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 19V5"></path>
                <path d="m5 12 7-7 7 7"></path>
            </svg>
        }
        .into_any(),
        _ => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="9"></circle>
                <path d="M12 7v5l3 2"></path>
            </svg>
        }
        .into_any(),
    }
}

struct NonAssistantMessageBodyParams {
    m_for_body: StoredMessage,
    is_tool_bubble: bool,
    tool_detail_text: Option<String>,
    tool_detail_open: RwSignal<bool>,
    locale: RwSignal<Locale>,
    chat_find_query: RwSignal<String>,
    apply_assistant_display_filters: RwSignal<bool>,
    jump_uid: Option<String>,
    auto_scroll_chat: RwSignal<bool>,
}

fn build_non_assistant_message_body(p: NonAssistantMessageBodyParams) -> AnyView {
    let NonAssistantMessageBodyParams {
        m_for_body,
        is_tool_bubble,
        tool_detail_text,
        tool_detail_open,
        locale,
        chat_find_query,
        apply_assistant_display_filters,
        jump_uid,
        auto_scroll_chat,
    } = p;
    if is_tool_bubble {
        let detail_for_btn = tool_detail_text;
        let tool_emoji = tool_bubble_emoji(&m_for_body);
        return view! {
            <div class="msg-tool-compact">
                <Show when=move || detail_for_btn.as_deref().is_some_and(|s| !s.trim().is_empty())>
                    <button
                        type="button"
                        class="msg-tool-drawer-btn msg-tool-drawer-icon-btn"
                        prop:title=move || {
                            if tool_detail_open.get() {
                                i18n::msg_tool_detail_collapse_title(locale.get())
                            } else {
                                i18n::msg_tool_detail_expand_title(locale.get())
                            }
                        }
                        prop:aria-label=move || {
                            if tool_detail_open.get() {
                                i18n::msg_tool_detail_collapse_title(locale.get())
                            } else {
                                i18n::msg_tool_detail_expand_title(locale.get())
                            }
                        }
                        on:click=move |_| {
                            tool_detail_open.update(|v| *v = !*v);
                        }
                    >
                        <svg
                            class=move || {
                                if tool_detail_open.get() {
                                    "msg-tool-drawer-icon is-open"
                                } else {
                                    "msg-tool-drawer-icon"
                                }
                            }
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <polyline points="6 9 12 15 18 9" />
                        </svg>
                    </button>
                </Show>
                <span class="msg-tool-emoji" aria-hidden="true">{tool_emoji}</span>
                <span class="msg-body msg-tool-summary">
                    {move || {
                        let apply = apply_assistant_display_filters.get();
                        let loc = locale.get();
                        let q = normalize_search_query(&chat_find_query.get());
                        render_highlighted_message_text(&m_for_body, loc, apply, &q)
                    }}
                </span>
            </div>
        }
        .into_any();
    }

    if let Some(uid) = jump_uid {
        let uid_click = uid.clone();
        let uid_key = uid.clone();
        return view! {
            <span
                class="msg-body msg-tool-body-jump"
                role="link"
                tabindex="0"
                prop:title=move || i18n::msg_jump_user_title(locale.get())
                prop:aria-label=move || i18n::msg_jump_user_aria(locale.get())
                on:click=move |_| {
                    spawn_scroll_to_linked_user_message(&uid_click, auto_scroll_chat);
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    let k = ev.key();
                    if k == "Enter" || k == " " {
                        ev.prevent_default();
                        spawn_scroll_to_linked_user_message(&uid_key, auto_scroll_chat);
                    }
                }
            >
                {move || {
                    let apply = apply_assistant_display_filters.get();
                    let loc = locale.get();
                    let q = normalize_search_query(&chat_find_query.get());
                    render_highlighted_message_text(&m_for_body, loc, apply, &q)
                }}
            </span>
        }
        .into_any();
    }

    view! {
        <span class="msg-body">
            {move || {
                let apply = apply_assistant_display_filters.get();
                let loc = locale.get();
                let q = normalize_search_query(&chat_find_query.get());
                render_highlighted_message_text(&m_for_body, loc, apply, &q)
            }}
        </span>
    }
    .into_any()
}

struct MessageActionsBarParams {
    show_msg_action_bar: bool,
    is_user_plain: bool,
    err: bool,
    msg_idx: usize,
    user_retry_id: String,
    user_branch_id: String,
    mid_retry: String,
    row_actions: MessageRowActionSignals,
    retry_assistant_target: RwSignal<Option<String>>,
    status_busy: RwSignal<bool>,
    locale: RwSignal<Locale>,
}

fn build_message_actions_bar(p: MessageActionsBarParams) -> AnyView {
    let MessageActionsBarParams {
        show_msg_action_bar,
        is_user_plain,
        err,
        msg_idx,
        user_retry_id,
        user_branch_id,
        mid_retry,
        row_actions,
        retry_assistant_target,
        status_busy,
        locale,
    } = p;
    if !show_msg_action_bar {
        return ().into_any();
    }

    view! {
        <div class="msg-actions msg-actions-below" role="group" prop:aria-label=move || i18n::msg_actions_group_aria(locale.get())>
            {is_user_plain.then(|| {
                let idx = msg_idx;
                let uid_r = user_retry_id.clone();
                let uid_b = user_branch_id.clone();
                view! {
                    <button
                        type="button"
                        class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                        prop:title=move || i18n::msg_regen_title(locale.get())
                        prop:aria-label=move || i18n::msg_regen_aria(locale.get())
                        prop:disabled=move || status_busy.get()
                        on:click=move |_| {
                            if status_busy.get() {
                                return;
                            }
                            row_actions.spawn_regenerate_from_user_line(
                                idx,
                                uid_r.clone(),
                            );
                        }
                    >
                        <svg
                            class="msg-action-icon"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            xmlns="http://www.w3.org/2000/svg"
                            aria-hidden="true"
                        >
                            <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                            <path d="M21 3v5h-5" />
                            <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                            <path d="M8 16H3v5" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                        prop:title=move || i18n::msg_branch_title(locale.get())
                        prop:aria-label=move || i18n::msg_branch_aria(locale.get())
                        prop:disabled=move || status_busy.get()
                        on:click=move |_| {
                            if status_busy.get() {
                                return;
                            }
                            row_actions.spawn_branch_at_user_line(
                                idx,
                                uid_b.clone(),
                            );
                        }
                    >
                        <svg
                            class="msg-action-icon"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            xmlns="http://www.w3.org/2000/svg"
                            aria-hidden="true"
                        >
                            <line x1="6" y1="3" x2="6" y2="15" fill="none" />
                            <circle cx="6" cy="3" r="2" fill="none" />
                            <path d="M6 15v-1a4 4 0 0 1 4-4h4a4 4 0 0 0 4-4V5" fill="none" />
                            <circle cx="18" cy="5" r="2" fill="none" />
                            <circle cx="18" cy="19" r="2" fill="none" />
                            <path d="M18 7v12" fill="none" />
                        </svg>
                    </button>
                }
            })}
            {err.then(move || {
                let mid = mid_retry.clone();
                view! {
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm msg-action-icon-btn"
                        prop:title=move || i18n::msg_retry_title(locale.get())
                        prop:aria-label=move || i18n::msg_retry_aria(locale.get())
                        prop:disabled=move || status_busy.get()
                        on:click=move |_| {
                            retry_assistant_target.set(Some(mid.clone()));
                        }
                    >
                        <svg
                            class="msg-action-icon"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            xmlns="http://www.w3.org/2000/svg"
                            aria-hidden="true"
                        >
                            <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                            <path d="M21 3v5h-5" />
                            <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                            <path d="M8 16H3v5" />
                        </svg>
                    </button>
                }
            })}
        </div>
    }
    .into_any()
}

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
                                prop:title=move || i18n::msg_copy_title(locale.get())
                                prop:aria-label=move || i18n::msg_copy_aria(locale.get())
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
                            let banner = i18n::msg_staged_timeline_exec_banner(locale.get());
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

#[cfg(test)]
mod tests {
    use super::is_running_subgoal_phase;
    use super::{
        build_subgoal_exec_banner_icon_key, build_subgoal_exec_banner_text,
        extract_hierarchical_goal_target,
    };
    use crate::i18n::{self, Locale};
    use crate::storage::{StoredMessage, StoredMessageState};

    fn subgoal_msg(text: &str) -> StoredMessage {
        StoredMessage {
            id: "m1".to_string(),
            role: "assistant".to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::HierarchicalSubgoal(
                "hierarchical-subgoal:goal_5".into(),
            )),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn extract_goal_target_from_subgoal_text() {
        let m = subgoal_msg("- 阶段：开始执行\n- 目标：创建 build 目录");
        let target = extract_hierarchical_goal_target(&m);
        assert_eq!(target.as_deref(), Some("创建 build 目录"));
    }

    #[test]
    fn build_exec_banner_for_started_phase() {
        let t = build_subgoal_exec_banner_text(
            Locale::ZhHans,
            Some("开始执行"),
            Some("创建 build 目录并运行 cmake"),
        );
        assert_eq!(t.as_deref(), Some("正在执行：创建 build 目录并运行 cmake…"));
    }

    #[test]
    fn build_exec_banner_for_fix_phase() {
        let t =
            build_subgoal_exec_banner_text(Locale::ZhHans, Some("修复"), Some("修正 CMake 路径"));
        assert_eq!(t.as_deref(), Some("正在修复：修正 CMake 路径…"));
    }

    #[test]
    fn build_exec_banner_icon_for_verify_phase() {
        let icon = build_subgoal_exec_banner_icon_key(Locale::ZhHans, Some("验证"));
        assert_eq!(icon, Some("verify"));
    }

    #[test]
    fn running_subgoal_phase_only_for_active_progress() {
        assert!(is_running_subgoal_phase(Locale::ZhHans, Some("修复")));
        assert!(!is_running_subgoal_phase(Locale::ZhHans, Some("完成")));
    }

    #[test]
    fn phase_key_is_locale_independent() {
        assert_eq!(
            i18n::hierarchical_subgoal_phase_key(Some("开始执行")),
            Some("run")
        );
        assert_eq!(
            i18n::hierarchical_subgoal_phase_key(Some("diagnose")),
            Some("diagnose")
        );
    }
}
