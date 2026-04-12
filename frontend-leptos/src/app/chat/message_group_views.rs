//! 连续工具输出折叠视图（[`super::message_row::chat_message_row`]）；分阶段时间线聚合为待办列表（[`super::staged_plan_todo`]）。

use std::collections::HashSet;

use leptos::prelude::*;

use super::message_row::chat_message_row;
use super::staged_plan_todo::{StagedStepPhase, build_staged_plan_todo_steps};
use crate::i18n::{self, Locale};
use crate::session_search::normalize_search_query;
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, StoredMessage};

#[allow(clippy::too_many_arguments)]
pub(crate) fn tool_run_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    expanded_tool_run_heads: RwSignal<HashSet<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    status_busy: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    auto_scroll_chat: RwSignal<bool>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let items_sv = StoredValue::new(items);
    let group_ids: Vec<String> = items_sv
        .get_value()
        .iter()
        .map(|(_, m)| m.id.clone())
        .collect();
    let n = items_sv.get_value().len();
    let head_for_expand_hint = head_key.clone();
    let head_attr = head_key.clone();
    let fold_head = head_key.clone();
    view! {
        <div class="msg-tool-run" data-tool-run=head_attr>
            {move || {
                let expanded_known =
                    expanded_tool_run_heads.with(|s| s.contains(&fold_head));
                let find_hit = {
                    let q = normalize_search_query(&chat_find_query.get());
                    !q.is_empty()
                        && chat_find_match_ids.with(|ids| {
                            ids
                                .iter()
                                .any(|mid| group_ids.iter().any(|g| g == mid))
                        })
                };
                let show_all = expanded_known || find_hit;
                let entries: Vec<_> = items_sv.get_value();
                let fold_on_click = fold_head.clone();
                let expand_on_click = head_for_expand_hint.clone();
                if show_all {
                    view! {
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_collapse_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_collapse_aria(locale.get())
                                on:click=move |_| {
                                    let k = fold_on_click.clone();
                                    expanded_tool_run_heads.update(|s| {
                                        s.remove(&k);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_collapse_btn(locale.get())}
                            </button>
                        </div>
                        {
                            entries
                                .into_iter()
                                .map(|(msg_idx, m)| {
                                    chat_message_row(
                                        msg_idx,
                                        m,
                                        sessions,
                                        active_id,
                                        collapsed_long_assistant_ids,
                                        chat_find_query,
                                        chat_find_match_ids,
                                        chat_find_cursor,
                                        auto_scroll_chat,
                                        status_busy,
                                        session_sync,
                                        regen_stream_after_truncate,
                                        retry_assistant_target,
                                        status_err,
                                        locale,
                                        markdown_render,
                                        apply_assistant_display_filters,
                                    )
                                })
                                .collect_view()
                        }
                    }
                    .into_any()
                } else if let Some((msg_idx, last)) = entries.last().cloned() {
                    view! {
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_expand_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_expand_aria(locale.get())
                                on:click=move |_| {
                                    let h = expand_on_click.clone();
                                    expanded_tool_run_heads.update(|s| {
                                        s.insert(h);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_expand_btn(locale.get())}
                            </button>
                        </div>
                        {chat_message_row(
                            msg_idx,
                            last,
                            sessions,
                            active_id,
                            collapsed_long_assistant_ids,
                            chat_find_query,
                            chat_find_match_ids,
                            chat_find_cursor,
                            auto_scroll_chat,
                            status_busy,
                            session_sync,
                            regen_stream_after_truncate,
                            retry_assistant_target,
                            status_err,
                            locale,
                            markdown_render,
                            apply_assistant_display_filters,
                        )}
                    }
                    .into_any()
                } else {
                    view! { <div class="msg-tool-run-empty"></div> }.into_any()
                }
            }}
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn staged_timeline_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
    session_messages: StoredValue<Vec<StoredMessage>>,
) -> impl IntoView {
    let items_sv = StoredValue::new(items);
    let group_ids: Vec<String> = items_sv
        .get_value()
        .iter()
        .map(|(_, m)| m.id.clone())
        .collect();
    let head_attr = head_key.clone();
    view! {
        <div class="msg-with-select">
            <div class="msg-stack">
                <div
                    class=move || {
                        let mut c =
                            String::from("msg msg-system msg-staged-timeline msg-staged-todo-wrap");
                        let q = normalize_search_query(&chat_find_query.get());
                        if !q.is_empty() {
                            let any = chat_find_match_ids.with(|ids| {
                                ids
                                    .iter()
                                    .any(|mid| group_ids.iter().any(|g| g == mid))
                            });
                            if any {
                                c.push_str(" msg-find-match");
                                let cur = chat_find_cursor.get();
                                if chat_find_match_ids.with(|ids| {
                                    ids
                                        .get(cur)
                                        .map(|x| group_ids.iter().any(|g| g == x))
                                        .unwrap_or(false)
                                }) {
                                    c.push_str(" msg-find-highlight");
                                }
                            }
                        }
                        c
                    }
                    prop:aria-label=move || i18n::staged_plan_todo_region_aria(locale.get())
                    data-staged-timeline-run=head_attr
                    data-testid="staged-plan-todo-card"
                >
                    {move || {
                        let _ = chat_find_cursor.get();
                        let _ = chat_find_match_ids.get();
                        let loc = locale.get();
                        let apply = apply_assistant_display_filters.get();
                        let entries = items_sv.get_value();
                        let session = session_messages.get_value();
                        let (steps, legacy) =
                            build_staged_plan_todo_steps(loc, apply, &entries, &session);
                        let title_bar = i18n::staged_plan_todo_title(loc);
                        view! {
                            <div class="msg-meta" aria-hidden="true">
                                <span class="msg-meta-primary msg-staged-todo-card-title">
                                    {title_bar}
                                </span>
                            </div>
                            <ul class="msg-staged-todo-list" role="list" data-testid="staged-plan-todo-list">
                                {steps
                                    .into_iter()
                                    .map(|s| {
                                        let anchor = s.anchor_message_id.clone();
                                        let anchor_cmp = anchor.clone();
                                        let row_id = format!("msg-{anchor}");
                                        let title = s.title.clone();
                                        let title_aria = title.clone();
                                        let li_phase = match s.phase {
                                            StagedStepPhase::Done => "msg-staged-todo-li-done",
                                            StagedStepPhase::InProgress => "msg-staged-todo-li-progress",
                                            StagedStepPhase::Pending => "msg-staged-todo-li-pending",
                                            StagedStepPhase::Cancelled => "msg-staged-todo-li-cancelled",
                                            StagedStepPhase::Failed => "msg-staged-todo-li-failed",
                                        };
                                        let glyph = match s.phase {
                                            StagedStepPhase::Done => "✓",
                                            StagedStepPhase::InProgress => "◦",
                                            StagedStepPhase::Pending => "☐",
                                            StagedStepPhase::Cancelled => "⊘",
                                            StagedStepPhase::Failed => "✗",
                                        };
                                        let phase_aria = match s.phase {
                                            StagedStepPhase::Done => {
                                                i18n::staged_plan_todo_step_done_aria(loc)
                                            }
                                            StagedStepPhase::InProgress => {
                                                i18n::staged_plan_todo_step_in_progress_aria(loc)
                                            }
                                            StagedStepPhase::Pending => {
                                                i18n::staged_plan_todo_step_pending_aria(loc)
                                            }
                                            StagedStepPhase::Failed => {
                                                i18n::staged_plan_todo_step_failed_aria(loc)
                                            }
                                            StagedStepPhase::Cancelled => {
                                                i18n::staged_plan_todo_step_cancelled_aria(loc)
                                            }
                                        };
                                        view! {
                                            <li
                                                class=move || {
                                                    let mut c = format!("msg-staged-todo-li {li_phase}");
                                                    let cur = chat_find_cursor.get();
                                                    if chat_find_match_ids.with(|ids| {
                                                        ids
                                                            .get(cur)
                                                            .map(|x| x.as_str() == anchor_cmp.as_str())
                                                            .unwrap_or(false)
                                                    }) {
                                                        c.push_str(" msg-find-highlight");
                                                    }
                                                    c
                                                }
                                                id=row_id
                                                role="listitem"
                                                prop:aria-label=format!("{phase_aria} · {title_aria}")
                                            >
                                                <span class="msg-staged-todo-glyph" aria-hidden="true">
                                                    {glyph}
                                                </span>
                                                <span class="msg-staged-todo-label">
                                                    {format!("{}. {}", s.ordinal, title)}
                                                </span>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                            {(!legacy.is_empty()).then(|| {
                                view! {
                                    <div class="msg-staged-todo-legacy" role="note">
                                        <span class="msg-staged-todo-legacy-label">
                                            {i18n::staged_plan_todo_legacy_note(loc)}
                                        </span>
                                        <ul class="msg-staged-todo-legacy-list">
                                            {legacy
                                                .into_iter()
                                                .map(|line| {
                                                    view! {
                                                        <li class="msg-staged-todo-legacy-li">{line}</li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </div>
                                }
                            })}
                        }
                        .into_any()
                    }}
                </div>
            </div>
        </div>
    }
}
