//! [`super::row::chat_message_row`] 的子视图与纯辅助逻辑，控制圈复杂度与物理行数。

use std::sync::Arc;

use leptos::prelude::*;

use crate::i18n::Locale;
use crate::session_ops::write_clipboard_text;
use crate::session_search::normalize_search_query;
use crate::storage::ChatSession;
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

use super::helpers::{
    hierarchical_subgoal_banner_is_active, message_row_loading_and_error,
    message_row_prefixed_class, stored_message_by_id,
};
use super::views::chat_message_row_subgoal_exec_banner_view;

pub(super) fn arc_retry_visible_for_message(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    mid: String,
) -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(move || {
        sessions.with(|list| {
            stored_message_by_id(list, active_id.get_untracked().as_str(), mid.as_str())
                .map(|msg| {
                    message_row_loading_and_error(
                        msg.is_tool,
                        msg.role.as_str(),
                        msg.state.as_ref(),
                    )
                    .1
                })
                .unwrap_or(false)
        })
    })
}

pub(super) fn arc_actions_bar_visible(
    is_tool_bubble: bool,
    is_user_plain: bool,
    retry_visible: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Arc<dyn Fn() -> bool + Send + Sync> {
    let retry = retry_visible.clone();
    Arc::new(move || !is_tool_bubble || is_user_plain || retry())
}

fn append_find_highlight_classes(
    out: &mut String,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    mid_highlight: &str,
) {
    let q = normalize_search_query(&chat_find_query.get());
    if q.is_empty() {
        return;
    }
    let in_list = chat_find_match_ids.with(|ids| ids.iter().any(|x| x == mid_highlight));
    if in_list {
        out.push_str(" msg-find-match");
    }
    let cur = chat_find_cursor.get();
    let is_current =
        chat_find_match_ids.with(|ids| ids.get(cur).map(|x| x == mid_highlight).unwrap_or(false));
    if is_current {
        out.push_str(" msg-find-highlight");
    }
}

pub(super) struct BubbleClassLiveCtx {
    pub cls: &'static str,
    pub is_tool_bubble: bool,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub mid_for_row: String,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
}

pub(super) fn bubble_css_classes_live(ctx: &BubbleClassLiveCtx) -> String {
    let (loading, err) = ctx.sessions.with(|list| {
        stored_message_by_id(
            list,
            ctx.active_id.get_untracked().as_str(),
            ctx.mid_for_row.as_str(),
        )
        .map(|msg| {
            message_row_loading_and_error(msg.is_tool, msg.role.as_str(), msg.state.as_ref())
        })
        .unwrap_or((false, false))
    });
    let mut c = message_row_prefixed_class(ctx.cls, err, loading);
    if !ctx.is_tool_bubble {
        c.push_str(" msg-has-inline-copy");
    }
    append_find_highlight_classes(
        &mut c,
        ctx.chat_find_query,
        ctx.chat_find_match_ids,
        ctx.chat_find_cursor,
        ctx.mid_for_row.as_str(),
    );
    c
}

pub(super) fn message_row_inline_copy_button(
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
    copy_id: String,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="btn btn-muted btn-sm msg-copy-inside-btn"
            prop:title=move || crate::i18n::msg_copy_title(locale.get())
            prop:aria-label=move || crate::i18n::msg_copy_aria(locale.get())
            on:click=move |_| {
                let loc = locale.get_untracked();
                let apply = apply_assistant_display_filters.get_untracked();
                let ov = stream_text_overlay.get_untracked();
                let t = sessions.with(|list| {
                    let aid = active_id.get_untracked();
                    list.iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| s.messages.iter().find(|msg| msg.id == copy_id))
                        .map(|msg| {
                            message_text_for_display_including_stream_overlay(
                                msg,
                                ov.as_ref(),
                                aid.as_str(),
                                loc,
                                apply,
                            )
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
}

pub(super) fn staged_timeline_exec_banner_when(locale: RwSignal<Locale>) -> impl IntoView {
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
            <span class="msg-subgoal-exec-banner-text">
                {move || crate::i18n::msg_staged_timeline_exec_banner(locale.get())}
            </span>
        </div>
    }
}

pub(super) struct SubgoalBannerReactiveCtx {
    pub locale: RwSignal<Locale>,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub mid_subgoal: String,
    pub phase_for_run_owned: Option<String>,
    pub subgoal_exec_banner: Option<String>,
    pub subgoal_exec_banner_icon_key: Option<&'static str>,
}

pub(super) fn subgoal_exec_banner_reactive_view(ctx: SubgoalBannerReactiveCtx) -> impl IntoView {
    let SubgoalBannerReactiveCtx {
        locale,
        sessions,
        active_id,
        mid_subgoal,
        phase_for_run_owned,
        subgoal_exec_banner,
        subgoal_exec_banner_icon_key,
    } = ctx;
    move || {
        let loc = locale.get();
        let phase_sl = phase_for_run_owned.as_deref();
        let active = sessions.with(|list| {
            hierarchical_subgoal_banner_is_active(
                list,
                active_id.get_untracked().as_str(),
                mid_subgoal.as_str(),
                subgoal_exec_banner.as_ref(),
                phase_sl,
                loc,
            )
        });
        chat_message_row_subgoal_exec_banner_view(
            subgoal_exec_banner.clone(),
            subgoal_exec_banner_icon_key,
            active,
        )
    }
}

pub(super) fn typing_dots_tail_assistant_row(
    tail_loading_assistant_mid: Memo<Option<String>>,
    row_message_id: String,
) -> impl IntoView {
    let mid_sv = StoredValue::new(row_message_id);
    view! {
        <Show when=move || {
            tail_loading_assistant_mid.get().as_deref() == Some(mid_sv.get_value().as_str())
        }>
            {move || {
                view! {
                    <span class="typing-dots typing-dots--model" aria-hidden="true">
                        <span></span>
                        <span></span>
                        <span></span>
                    </span>
                }
            }}
        </Show>
    }
}
