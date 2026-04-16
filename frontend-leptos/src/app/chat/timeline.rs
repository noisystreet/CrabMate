//! 聊天列内可折叠的「规划 / 工具」时间线索引（跳转至对应消息）。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app_prefs::{TIMELINE_PANEL_EXPANDED_KEY, load_bool_key, store_bool_key};
use crate::i18n::{self, Locale};
use crate::session_search::scroll_message_into_view;
use crate::storage::ChatSession;
use crate::timeline_scan::{TimelineKind, collect_timeline_entries, timeline_entry_is_failed};

fn label_for_entry(loc: Locale, kind: &TimelineKind) -> String {
    match kind {
        TimelineKind::StagedStart {
            step_index,
            total_steps: _,
        } => i18n::timeline_panel_staged_start(loc, *step_index),
        TimelineKind::StagedEnd {
            step_index,
            total_steps: _,
            status,
        } => i18n::timeline_panel_staged_end(loc, *step_index, status),
        TimelineKind::Tool { ok } => i18n::timeline_panel_tool(loc, *ok),
        TimelineKind::ApprovalDecision { decision } => {
            i18n::timeline_panel_approval_decision(loc, decision)
        }
        TimelineKind::LegacyStaged => i18n::timeline_panel_legacy_staged(loc).to_string(),
        TimelineKind::LegacyTool => i18n::timeline_panel_legacy_tool(loc).to_string(),
    }
}

fn jump_to_message(msg_id: &str, auto_scroll_chat: RwSignal<bool>) {
    auto_scroll_chat.set(false);
    let id = msg_id.to_string();
    spawn_local(async move {
        TimeoutFuture::new(32).await;
        scroll_message_into_view(&id);
    });
}

#[allow(clippy::too_many_arguments)]
pub fn timeline_panel_view(
    locale: RwSignal<Locale>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    timeline_panel_expanded: RwSignal<bool>,
    auto_scroll_chat: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="timeline-panel-wrap">
            <button
                type="button"
                class="timeline-panel-toggle btn btn-muted btn-sm"
                aria-expanded=move || timeline_panel_expanded.get()
                prop:title=move || i18n::timeline_panel_toggle_title(locale.get())
                prop:aria-label=move || i18n::timeline_panel_toggle_aria(locale.get())
                on:click=move |_| {
                    let next = !timeline_panel_expanded.get();
                    timeline_panel_expanded.set(next);
                    store_bool_key(TIMELINE_PANEL_EXPANDED_KEY, next);
                }
            >
                <svg
                    class="timeline-panel-toggle-icon"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M12 20v-8" />
                    <path d="M12 4v2" />
                    <circle cx="12" cy="12" r="7" />
                </svg>
                <span class="timeline-panel-toggle-text">
                    {move || i18n::timeline_panel_toggle_label(locale.get())}
                </span>
                {move || {
                    let id = active_id.get();
                    let n = sessions.with(|list| {
                        list.iter()
                            .find(|s| s.id == id)
                            .map(|s| collect_timeline_entries(&s.messages).len())
                            .unwrap_or(0)
                    });
                    if n == 0 {
                        view! { <span></span> }.into_any()
                    } else {
                        view! {
                            <span class="timeline-panel-count" aria-hidden="true">
                                {format!("{n}")}
                            </span>
                        }
                        .into_any()
                    }
                }}
            </button>
            <Show when=move || timeline_panel_expanded.get()>
                <div
                    class="timeline-panel"
                    role="region"
                    prop:aria-label=move || i18n::timeline_panel_region_aria(locale.get())
                >
                    {move || {
                        let loc = locale.get();
                        let id = active_id.get();
                        let entries = sessions.with(|list| {
                            list.iter()
                                .find(|s| s.id == id)
                                .map(|s| collect_timeline_entries(&s.messages))
                                .unwrap_or_default()
                        });
                        if entries.is_empty() {
                            view! {
                                <p class="timeline-panel-empty" role="status">
                                    {i18n::timeline_panel_empty(loc)}
                                </p>
                            }
                            .into_any()
                        } else {
                            view! {
                                <ul class="timeline-panel-list">
                                    {entries
                                        .into_iter()
                                        .map(|e| {
                                            let mid = e.message_id.clone();
                                            let mid_click = mid.clone();
                                            let mid_key = mid.clone();
                                            let failed = timeline_entry_is_failed(&e.kind);
                                            let lbl = label_for_entry(loc, &e.kind);
                                            let lbl_aria = lbl.clone();
                                            let item_cls = if failed {
                                                "timeline-panel-item timeline-panel-item-failed"
                                            } else {
                                                "timeline-panel-item"
                                            };
                                            view! {
                                                <li class=item_cls>
                                                    <button
                                                        type="button"
                                                        class="timeline-panel-jump"
                                                        prop:title=move || {
                                                            i18n::timeline_panel_jump_title(locale.get())
                                                        }
                                                        prop:aria-label=move || {
                                                            i18n::timeline_panel_jump_aria(
                                                                locale.get(),
                                                                &lbl_aria,
                                                            )
                                                        }
                                                        on:click=move |_| {
                                                            jump_to_message(&mid_click, auto_scroll_chat);
                                                        }
                                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                            let k = ev.key();
                                                            if k == "Enter" || k == " " {
                                                                ev.prevent_default();
                                                                jump_to_message(&mid_key, auto_scroll_chat);
                                                            }
                                                        }
                                                    >
                                                        <span class="timeline-panel-dot" aria-hidden="true"></span>
                                                        <span class="timeline-panel-label">{lbl}</span>
                                                    </button>
                                                </li>
                                            }
                                            .into_any()
                                        })
                                        .collect_view()}
                                </ul>
                            }
                            .into_any()
                        }
                    }}
                </div>
            </Show>
        </div>
    }
}

pub fn load_timeline_panel_expanded_default() -> bool {
    load_bool_key(TIMELINE_PANEL_EXPANDED_KEY, false)
}
