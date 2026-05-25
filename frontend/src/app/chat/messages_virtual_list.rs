//! 消息列：分页「加载更早」+ chunk 虚拟窗口。

use leptos::prelude::*;

use super::message_chunks::{ChatChunk, chat_chunk_stable_key, chunk_messages};
use super::message_group_views::{ToolRunGroupSignals, tool_run_group_view};
use super::message_row::{ChatMessageRowSignals, chat_message_row};
use super::message_virtual_viewport::{
    should_virtualize_chunks_for_stream_follow, tail_virtual_chunk_range, virtual_spacer_heights,
};
use super::messages_scroll_compensate::LoadOlderScrollContext;
use super::session_hydrate::try_load_older_messages_for_active_session;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::storage::ChatSession;

#[derive(Clone, Copy)]
pub(crate) struct ChatMessagesVirtualListSignals {
    pub chat: ChatSessionSignals,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub locale: RwSignal<crate::i18n::Locale>,
    pub virtual_scroll_top: RwSignal<i32>,
    pub virtual_viewport_height: RwSignal<i32>,
    pub messages_scroller: NodeRef<leptos::html::Div>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub tool_run_group_signals: ToolRunGroupSignals,
}

#[component]
pub(crate) fn ChatMessagesVirtualList(signals: ChatMessagesVirtualListSignals) -> impl IntoView {
    let ChatMessagesVirtualListSignals {
        chat,
        sessions,
        active_id,
        locale,
        virtual_scroll_top,
        virtual_viewport_height,
        messages_scroller,
        messages_scroll_from_effect,
        tool_run_group_signals,
    } = signals;
    let ToolRunGroupSignals {
        auto_scroll_chat,
        tail_loading_assistant_mid,
        collapsed_long_assistant_ids,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        stream_turn_busy_ui,
        stream_follow_up,
        status_err,
        markdown_render,
        apply_assistant_display_filters,
        tool_detail_expanded_ids,
        ..
    } = tool_run_group_signals;

    let render_chunk = move |chunk: ChatChunk| match chunk {
        ChatChunk::Single { idx, msg } => chat_message_row(ChatMessageRowSignals {
            msg_idx: idx,
            m: msg,
            chat,
            collapsed_long_assistant_ids,
            chat_find_query,
            chat_find_match_ids,
            chat_find_cursor,
            auto_scroll_chat,
            stream_turn_busy_ui,
            tail_loading_assistant_mid,
            stream_follow_up,
            status_err,
            locale,
            markdown_render,
            apply_assistant_display_filters,
            tool_detail_expanded_ids,
        })
        .into_any(),
        ChatChunk::ToolGroup { head_id, items } => {
            tool_run_group_view(head_id, items, tool_run_group_signals).into_any()
        }
    };

    move || {
        let id = active_id.get();
        let (chunks, has_older, loading_older) = sessions.with(|list| {
            let Some(s) = list.iter().find(|s| s.id == id) else {
                return (Vec::new(), false, false);
            };
            (
                chunk_messages(&s.messages),
                s.history_has_older_flag(),
                chat.history_loading_older.get_untracked(),
            )
        });
        let count = chunks.len();
        let follow_virtual =
            should_virtualize_chunks_for_stream_follow(count, auto_scroll_chat.get_untracked());
        let (top_pad, bottom_pad, slice) = if follow_virtual {
            let viewport_h = virtual_viewport_height.get();
            let range = tail_virtual_chunk_range(count, viewport_h);
            let (top_pad, bottom_pad) = virtual_spacer_heights(range, count);
            (top_pad, bottom_pad, chunks[range.start..range.end].to_vec())
        } else {
            (0, 0, chunks)
        };
        view! {
            <Show when=move || has_older || loading_older>
                <div class="messages-history-load" role="status">
                    <Show
                        when=move || loading_older
                        fallback=move || {
                            view! {
                                <button
                                    type="button"
                                    class="btn btn-ghost btn-sm"
                                    data-testid="chat-load-older"
                                    on:click=move |_| {
                                        try_load_older_messages_for_active_session(
                                            chat,
                                            locale.get_untracked(),
                                            LoadOlderScrollContext::capture(
                                                messages_scroller,
                                                messages_scroll_from_effect,
                                                virtual_scroll_top,
                                                virtual_viewport_height,
                                            ),
                                        );
                                    }
                                >
                                    {move || i18n::chat_history_load_older(locale.get())}
                                </button>
                            }
                        }
                    >
                        <span class="messages-history-load-busy">
                            {move || i18n::chat_history_loading_older(locale.get())}
                        </span>
                    </Show>
                </div>
            </Show>
            <div class="messages-virtual-top" style:height=format!("{top_pad}px")></div>
            <For
                each=move || slice.clone()
                key=|chunk| chat_chunk_stable_key(chunk)
                children=move |chunk| render_chunk(chunk)
            />
            <div class="messages-virtual-bottom" style:height=format!("{bottom_pad}px")></div>
        }
    }
}
