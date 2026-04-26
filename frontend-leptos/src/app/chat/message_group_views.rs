//! 连续工具输出组视图（[`super::message_row::chat_message_row`]）：默认展开全部，可收起为仅最后一条；
//! 组内顺序与 [`super::message_chunks::chunk_messages`] 传入的 `items` 一致（即会话里消息到达/排列顺序，通常旧在上、新在下），标题栏在组内底部。

use std::collections::HashSet;

use leptos::prelude::*;

use super::message_row::chat_message_row;
use crate::i18n::{self, Locale};
use crate::session_search::normalize_search_query;
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, StoredMessage};

#[allow(clippy::too_many_arguments)]
pub(crate) fn tool_run_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    collapsed_tool_run_heads: RwSignal<HashSet<String>>,
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
                let folded_to_last =
                    collapsed_tool_run_heads.with(|s| s.contains(&fold_head));
                let find_hit = {
                    let q = normalize_search_query(&chat_find_query.get());
                    !q.is_empty()
                        && chat_find_match_ids.with(|ids| {
                            ids
                                .iter()
                                .any(|mid| group_ids.iter().any(|g| g == mid))
                        })
                };
                let show_all = !folded_to_last || find_hit;
                let entries: Vec<_> = items_sv.get_value();
                let fold_on_click = fold_head.clone();
                let expand_on_click = head_for_expand_hint.clone();
                if show_all {
                    view! {
                        {
                            entries
                                .iter()
                                .map(|(msg_idx, m)| {
                                    chat_message_row(
                                        *msg_idx,
                                        m.clone(),
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
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_collapse_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_collapse_aria(locale.get())
                                on:click=move |_| {
                                    let k = fold_on_click.clone();
                                    collapsed_tool_run_heads.update(|s| {
                                        s.insert(k);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_collapse_btn(locale.get())}
                            </button>
                        </div>
                    }
                    .into_any()
                } else if let Some((msg_idx, last)) = entries.last().cloned() {
                    view! {
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
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_expand_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_expand_aria(locale.get())
                                on:click=move |_| {
                                    let h = expand_on_click.clone();
                                    collapsed_tool_run_heads.update(|s| {
                                        s.remove(&h);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_expand_btn(locale.get())}
                            </button>
                        </div>
                    }
                    .into_any()
                } else {
                    view! { <div class="msg-tool-run-empty"></div> }.into_any()
                }
            }}
        </div>
    }
}
