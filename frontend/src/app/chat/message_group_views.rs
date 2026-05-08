//! 连续工具输出组视图（[`super::message_row::chat_message_row`]）：默认展开全部，可收起为仅最后一条；
//! 组内顺序与 [`super::message_chunks::chunk_messages`] 传入的 `items` 一致（即会话里消息到达/排列顺序，通常旧在上、新在下），标题栏在组内底部。

use std::collections::HashSet;

use leptos::prelude::*;

use super::message_row::{ChatMessageRowSignals, chat_message_row};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_search::normalize_search_query;
use crate::storage::StoredMessage;

/// 工具组内每条 [`chat_message_row`] 共享的信号（缩短 [`tool_run_group_view`] 形参列表）。
#[derive(Clone, Copy)]
pub(crate) struct ToolRunGroupSignals {
    pub collapsed_tool_run_heads: RwSignal<HashSet<String>>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat: ChatSessionSignals,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub stream_turn_busy_ui: Memo<bool>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub status_err: RwSignal<Option<String>>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

fn chat_row_for_tool_group(
    msg_idx: usize,
    m: StoredMessage,
    g: ToolRunGroupSignals,
) -> impl IntoView {
    chat_message_row(ChatMessageRowSignals {
        msg_idx,
        m,
        chat: g.chat,
        collapsed_long_assistant_ids: g.collapsed_long_assistant_ids,
        chat_find_query: g.chat_find_query,
        chat_find_match_ids: g.chat_find_match_ids,
        chat_find_cursor: g.chat_find_cursor,
        auto_scroll_chat: g.auto_scroll_chat,
        stream_turn_busy_ui: g.stream_turn_busy_ui,
        regen_stream_after_truncate: g.regen_stream_after_truncate,
        retry_assistant_target: g.retry_assistant_target,
        status_err: g.status_err,
        locale: g.locale,
        markdown_render: g.markdown_render,
        apply_assistant_display_filters: g.apply_assistant_display_filters,
    })
}

pub(crate) fn tool_run_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    g: ToolRunGroupSignals,
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
                    g.collapsed_tool_run_heads.with(|s| s.contains(&fold_head));
                let find_hit = {
                    let q = normalize_search_query(&g.chat_find_query.get());
                    !q.is_empty()
                        && g.chat_find_match_ids.with(|ids| {
                            ids
                                .iter()
                                .any(|mid| group_ids.iter().any(|g_id| g_id == mid))
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
                                    chat_row_for_tool_group(*msg_idx, m.clone(), g)
                                })
                                .collect_view()
                        }
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(g.locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(g.locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_collapse_title(g.locale.get())
                                prop:aria-label=move || i18n::msg_tool_collapse_aria(g.locale.get())
                                on:click=move |_| {
                                    let k = fold_on_click.clone();
                                    g.collapsed_tool_run_heads.update(|s| {
                                        s.insert(k);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_collapse_btn(g.locale.get())}
                            </button>
                        </div>
                    }
                    .into_any()
                } else if let Some((msg_idx, last)) = entries.last().cloned() {
                    view! {
                        {chat_row_for_tool_group(msg_idx, last, g)}
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(g.locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(g.locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_expand_title(g.locale.get())
                                prop:aria-label=move || i18n::msg_tool_expand_aria(g.locale.get())
                                on:click=move |_| {
                                    let h = expand_on_click.clone();
                                    g.collapsed_tool_run_heads.update(|s| {
                                        s.remove(&h);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_expand_btn(g.locale.get())}
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
