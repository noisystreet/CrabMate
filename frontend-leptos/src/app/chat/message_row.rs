//! 单条消息气泡与下方操作条（复制 / 重试 / 分支等）。
//!
//! 与 `POST /chat/branch`、本地截断再生相关的副作用见 [`super::message_row_actions`]。

use leptos::prelude::*;

use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::i18n::{self, Locale};
use crate::message_format::{
    is_staged_timeline_stored_message, message_text_for_display_ex,
    stored_message_is_staged_planner_round,
};
use crate::session_ops::{
    format_msg_time_label, message_role_label, preceding_plain_user_message_id,
    write_clipboard_text,
};
use crate::session_search::{normalize_search_query, split_for_find_highlight};
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, StoredMessage};

use super::message_row_actions::{MessageRowActionSignals, spawn_scroll_to_linked_user_message};

fn extract_hierarchical_phase_chip(msg: &StoredMessage) -> Option<(String, String)> {
    let state = msg.state.as_deref()?;
    if !state.starts_with("hierarchical-subgoal:") {
        return None;
    }
    let phase = msg.text.lines().map(str::trim).find_map(|line| {
        line.strip_prefix("- 阶段：")
            .or_else(|| line.strip_prefix("阶段："))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })?;
    let cls = match phase.as_str() {
        "诊断" => "msg-subgoal-phase-chip phase-diagnose",
        "修复" => "msg-subgoal-phase-chip phase-fix",
        "验证" => "msg-subgoal-phase-chip phase-verify",
        "升级" => "msg-subgoal-phase-chip phase-escalate",
        _ => "msg-subgoal-phase-chip",
    };
    Some((phase, cls.to_string()))
}

fn extract_hierarchical_metrics(msg: &StoredMessage) -> Option<String> {
    let state = msg.state.as_deref()?;
    if !state.starts_with("hierarchical-subgoal:") {
        return None;
    }
    let mut error_count: Option<String> = None;
    let mut stagnant_rounds: Option<String> = None;
    for line in msg.text.lines().map(str::trim) {
        if error_count.is_none()
            && let Some(v) = line
                .strip_prefix("- 错误数：")
                .or_else(|| line.strip_prefix("错误数："))
        {
            let v = v.trim();
            if !v.is_empty() {
                error_count = Some(v.to_string());
            }
        }
        if stagnant_rounds.is_none()
            && let Some(v) = line
                .strip_prefix("- 无进展轮次：")
                .or_else(|| line.strip_prefix("无进展轮次："))
        {
            let v = v.trim();
            if !v.is_empty() {
                stagnant_rounds = Some(v.to_string());
            }
        }
    }
    if error_count.is_none() && stagnant_rounds.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(v) = error_count {
        parts.push(format!("错误数 {v}"));
    }
    if let Some(v) = stagnant_rounds {
        parts.push(format!("无进展 {v} 轮"));
    }
    Some(parts.join(" · "))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn chat_message_row(
    msg_idx: usize,
    m: StoredMessage,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    auto_scroll_chat: RwSignal<bool>,
    status_busy: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let row_actions = MessageRowActionSignals {
        session_sync,
        sessions,
        active_id,
        regen_stream_after_truncate,
        status_err,
        locale,
    };
    let is_staged_timeline = is_staged_timeline_stored_message(&m);
    let cls = if is_staged_timeline {
        "msg msg-system msg-staged-timeline"
    } else {
        match m.role.as_str() {
            "user" => "msg msg-user",
            "assistant" if m.is_tool => "msg msg-tool",
            "assistant" => "msg msg-assistant",
            _ if m.is_tool => "msg msg-tool",
            _ => "msg msg-system",
        }
    };
    let loading = (m.role == "assistant" && m.state.as_deref() == Some("loading"))
        || (m.is_tool && m.state.as_deref() == Some("loading"));
    let err = m.state.as_deref() == Some("error");
    let class_prefix = if err {
        format!("{cls} msg-error")
    } else if loading {
        format!("{cls} msg-loading")
    } else {
        cls.to_string()
    };
    let mid_highlight = m.id.clone();
    let m_role = m.clone();
    let role_lbl = move || message_role_label(&m_role, locale.get());
    let time_str = format_msg_time_label(m.created_at).unwrap_or_default();
    let mid_retry = m.id.clone();
    let copy_id = m.id.clone();
    let user_retry_id = m.id.clone();
    let user_branch_id = m.id.clone();
    let is_user_plain = m.role == "user" && !m.is_tool;
    let is_tool_bubble = m.is_tool;
    let tool_detail_open = RwSignal::new(false);
    let jump_uid = if is_tool_bubble {
        sessions.with(|list| {
            let aid = active_id.get();
            list.iter()
                .find(|s| s.id == aid)
                .and_then(|sess| preceding_plain_user_message_id(&sess.messages, msg_idx))
        })
    } else {
        None
    };
    let show_msg_action_bar = !is_tool_bubble || is_user_plain || err;
    let show_planner_round_badge = stored_message_is_staged_planner_round(&m);
    let subgoal_phase_chip = extract_hierarchical_phase_chip(&m);
    let subgoal_metrics_line = extract_hierarchical_metrics(&m);
    let msg_core = if m.role == "assistant" && !m.is_tool {
        assistant_markdown_collapsible_view(
            sessions,
            active_id,
            m.id.clone(),
            collapsed_long_assistant_ids,
            locale,
            markdown_render,
            apply_assistant_display_filters,
        )
        .into_any()
    } else {
        let m_for_body = m.clone();
        let asc = auto_scroll_chat;
        // 工具气泡不需要跳转到用户消息的功能，只显示纯文本
        let body_inner = if is_tool_bubble {
            let detail_text = m_for_body.reasoning_text.clone();
            let detail_for_btn = detail_text.clone();
            let detail_for_show = detail_text.clone();
            view! {
                <div class="msg-tool-compact">
                    <span class="msg-body msg-tool-summary">
                        {move || {
                            let apply = apply_assistant_display_filters.get();
                            let loc = locale.get();
                            let q = normalize_search_query(&chat_find_query.get());
                            let disp =
                                message_text_for_display_ex(&m_for_body, loc, apply);
                            let segs = split_for_find_highlight(&disp, &q);
                            segs
                                .into_iter()
                                .map(|(s, hl)| {
                                    if hl {
                                        view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
                                    } else {
                                        view! { {s} }.into_any()
                                    }
                                })
                                .collect_view()
                        }}
                    </span>
                    <Show when=move || !detail_for_btn.trim().is_empty()>
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
                </div>
                <Show when=move || tool_detail_open.get() && !detail_for_show.trim().is_empty()>
                    <div class="msg-tool-drawer">
                        <pre class="msg-tool-drawer-pre">{detail_text.clone()}</pre>
                    </div>
                </Show>
            }
            .into_any()
        } else if let Some(uid) = jump_uid {
            let uid_click = uid.clone();
            let uid_key = uid.clone();
            view! {
                <span
                    class="msg-body msg-tool-body-jump"
                    role="link"
                    tabindex="0"
                    prop:title=move || i18n::msg_jump_user_title(locale.get())
                    prop:aria-label=move || i18n::msg_jump_user_aria(locale.get())
                    on:click=move |_| {
                        spawn_scroll_to_linked_user_message(&uid_click, asc);
                    }
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        let k = ev.key();
                        if k == "Enter" || k == " " {
                            ev.prevent_default();
                            spawn_scroll_to_linked_user_message(&uid_key, asc);
                        }
                    }
                >
                    {move || {
                        let apply = apply_assistant_display_filters.get();
                        let loc = locale.get();
                        let q = normalize_search_query(&chat_find_query.get());
                        let disp =
                            message_text_for_display_ex(&m_for_body, loc, apply);
                        let segs = split_for_find_highlight(&disp, &q);
                        segs
                            .into_iter()
                            .map(|(s, hl)| {
                                if hl {
                                    view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
                                } else {
                                    view! { {s} }.into_any()
                                }
                            })
                            .collect_view()
                    }}
                </span>
            }
            .into_any()
        } else {
            view! {
                <span class="msg-body">
                    {move || {
                        let apply = apply_assistant_display_filters.get();
                        let loc = locale.get();
                        let q = normalize_search_query(&chat_find_query.get());
                        let disp =
                            message_text_for_display_ex(&m_for_body, loc, apply);
                        let segs = split_for_find_highlight(&disp, &q);
                        segs
                            .into_iter()
                            .map(|(s, hl)| {
                                if hl {
                                    view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
                                } else {
                                    view! { {s} }.into_any()
                                }
                            })
                            .collect_view()
                    }}
                </span>
            }
            .into_any()
        };
        if is_user_plain && !m.image_urls.is_empty() {
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
    };
    let mid_dom = m.id.clone();
    view! {
        <div class="msg-with-select">
            <div class="msg-stack">
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
                    <span class="msg-meta-time">{time_str}</span>
                </div>
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
                    {subgoal_phase_chip.as_ref().map(|(phase, chip_class)| {
                        let phase = phase.clone();
                        let chip_class = chip_class.clone();
                        view! {
                            <div class=chip_class>{phase}</div>
                        }
                    })}
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
                {show_msg_action_bar.then(|| {
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
                                            <line
                                                x1="6"
                                                y1="3"
                                                x2="6"
                                                y2="15"
                                                fill="none"
                                            />
                                            <circle cx="6" cy="3" r="2" fill="none" />
                                            <path
                                                d="M6 15v-1a4 4 0 0 1 4-4h4a4 4 0 0 0 4-4V5"
                                                fill="none"
                                            />
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
                })}
            </div>
        </div>
    }
}
