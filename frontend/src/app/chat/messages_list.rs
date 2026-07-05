//! 消息列：全量 chunk 渲染与「加载更早」按钮。

use std::collections::HashMap;

use leptos::prelude::*;

use super::message_chunks::{ChatChunk, chat_chunk_stable_key, chunk_messages};
use super::message_group_views::{ToolRunGroupSignals, tool_run_group_view};
use super::message_row::helpers::message_row_loading_and_error;
use super::message_row::{ChatMessageRowSignals, chat_message_row};
use super::scroll_shell::ChatScrollShellSignals;
use super::session_hydrate::try_load_older_messages_for_active_session;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::storage::ChatSession;

#[derive(Clone, Copy)]
pub(crate) struct ChatMessagesListSignals {
    pub chat: ChatSessionSignals,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub locale: RwSignal<crate::i18n::Locale>,
    pub scroll_shell: ChatScrollShellSignals,
    pub tool_run_group_signals: ToolRunGroupSignals,
}

#[component]
pub(crate) fn ChatMessagesList(signals: ChatMessagesListSignals) -> impl IntoView {
    let ChatMessagesListSignals {
        chat,
        sessions,
        active_id,
        locale,
        scroll_shell,
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

    let row_state_map: Memo<HashMap<String, (bool, bool)>> = Memo::new(move |_| {
        let id = active_id.get();
        sessions.with(|list| {
            let mut map = HashMap::new();
            if let Some(s) = list.iter().find(|s| s.id == id) {
                for msg in &s.messages {
                    let state = message_row_loading_and_error(
                        msg.is_tool,
                        msg.role.as_str(),
                        msg.state.as_ref(),
                    );
                    map.insert(msg.id.clone(), state);
                }
            }
            map
        })
    });

    let tool_run_group_signals_with_map = ToolRunGroupSignals {
        row_state_map,
        ..tool_run_group_signals
    };

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
            row_state_map,
        })
        .into_any(),
        ChatChunk::ToolGroup { head_id, items } => {
            tool_run_group_view(head_id, items, tool_run_group_signals_with_map).into_any()
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
                                            scroll_shell,
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
            <For
                each=move || chunks.clone()
                key=|chunk| chat_chunk_stable_key(chunk)
                children=move |chunk| render_chunk(chunk)
            />
        }
    }
}
