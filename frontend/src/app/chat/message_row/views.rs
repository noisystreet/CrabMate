//! 消息行内部子视图：元信息、正文、子目标横幅、工具气泡与操作条。

use std::sync::Arc;

use leptos::prelude::*;

use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::i18n::{self, Locale};
use crate::session_ops::message_role_label;
use crate::storage::{ChatSession, StoredMessage};
use crate::stream_text_overlay::StreamTextOverlay;

use super::super::message_row_actions::MessageRowActionSignals;
use super::non_assistant_body::{NonAssistantMessageBodyParams, build_non_assistant_message_body};

pub(super) fn chat_message_row_meta_view(
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

pub(super) struct ChatMessageRowBodyCoreParams {
    pub m: StoredMessage,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub chat_find_query: RwSignal<String>,
    pub is_tool_bubble: bool,
    pub tool_detail_text: Option<String>,
    pub tool_detail_open: RwSignal<bool>,
    pub jump_uid: Option<String>,
    pub auto_scroll_chat: RwSignal<bool>,
}

pub(super) fn chat_message_row_body_core(p: ChatMessageRowBodyCoreParams) -> AnyView {
    let ChatMessageRowBodyCoreParams {
        m,
        sessions,
        active_id,
        stream_text_overlay,
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
            stream_text_overlay,
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

pub(super) fn chat_message_row_subgoal_exec_banner_view(
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

fn user_line_regen_branch_buttons(
    msg_idx: usize,
    uid_r: String,
    uid_b: String,
    row_actions: MessageRowActionSignals,
    stream_turn_busy_ui: Memo<bool>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
            prop:title=move || i18n::msg_regen_title(locale.get())
            prop:aria-label=move || i18n::msg_regen_aria(locale.get())
            prop:disabled=move || stream_turn_busy_ui.get()
            on:click=move |_| {
                if stream_turn_busy_ui.get() {
                    return;
                }
                row_actions.spawn_regenerate_from_user_line(msg_idx, uid_r.clone());
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
            prop:disabled=move || stream_turn_busy_ui.get()
            on:click=move |_| {
                if stream_turn_busy_ui.get() {
                    return;
                }
                row_actions.spawn_branch_at_user_line(msg_idx, uid_b.clone());
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
}

fn assistant_retry_icon_button(
    retry_assistant_target: RwSignal<Option<String>>,
    stream_turn_busy_ui: Memo<bool>,
    locale: RwSignal<Locale>,
    mid_retry_go: StoredValue<String>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="btn btn-secondary btn-sm msg-action-icon-btn"
            prop:title=move || i18n::msg_retry_title(locale.get())
            prop:aria-label=move || i18n::msg_retry_aria(locale.get())
            prop:disabled=move || stream_turn_busy_ui.get()
            on:click=move |_| {
                retry_assistant_target.set(Some(mid_retry_go.get_value()));
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
}

pub(super) struct MessageActionsBarParams {
    /// 是否渲染整条操作条（含用户再生成等）；流式保留 DOM 时需每次读会话快照。
    pub actions_bar_visible: Arc<dyn Fn() -> bool + Send + Sync>,
    pub is_user_plain: bool,
    pub retry_visible: Arc<dyn Fn() -> bool + Send + Sync>,
    pub msg_idx: usize,
    pub user_retry_id: String,
    pub user_branch_id: String,
    pub mid_retry: String,
    pub row_actions: MessageRowActionSignals,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub stream_turn_busy_ui: Memo<bool>,
    pub locale: RwSignal<Locale>,
}

pub(super) fn build_message_actions_bar(p: MessageActionsBarParams) -> AnyView {
    let MessageActionsBarParams {
        actions_bar_visible,
        is_user_plain,
        retry_visible,
        msg_idx,
        user_retry_id,
        user_branch_id,
        mid_retry,
        row_actions,
        retry_assistant_target,
        stream_turn_busy_ui,
        locale,
    } = p;

    let bar_vis = StoredValue::new(actions_bar_visible);
    let retry_check = StoredValue::new(retry_visible);
    let mid_retry_go = StoredValue::new(mid_retry);

    view! {
        <Show when=move || bar_vis.get_value()()>
            <div class="msg-actions msg-actions-below" role="group" prop:aria-label=move || i18n::msg_actions_group_aria(locale.get())>
            {is_user_plain.then(|| {
                user_line_regen_branch_buttons(
                    msg_idx,
                    user_retry_id.clone(),
                    user_branch_id.clone(),
                    row_actions,
                    stream_turn_busy_ui,
                    locale,
                )
            })}
            <Show when=move || retry_check.get_value()()>
                {assistant_retry_icon_button(
                    retry_assistant_target,
                    stream_turn_busy_ui,
                    locale,
                    mid_retry_go,
                )}
            </Show>
            </div>
        </Show>
    }
    .into_any()
}
