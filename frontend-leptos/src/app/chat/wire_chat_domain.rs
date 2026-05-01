//! 聊天域接线：会话切换、草稿同步、滚底、查找、composer 流式等 `wire_*` 从 `app/mod.rs` 迁出，落实 **`docs/frontend-leptos/ARCHITECTURE.md`** 阶段 **B（壳与域分离）**。
//!
//! `App` 仍负责创建跨域共享的 `RwSignal`（如 `status_busy`）并组装 [`ComposerStreamShell`](super::handles::ComposerStreamShell)；本模块只注册聊天相关副作用并返回 [`ChatComposerWires`](super::composer::ChatComposerWires)。

use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;
use crate::storage::ChatSession;

use super::composer::{
    ChatComposerWires, wire_chat_composer_streams, wire_draft_sync_to_buffer_and_textarea,
    wire_session_switch_clears_chat_state,
};
use super::find::wire_chat_find_matches;
use super::handles::{ComposerStreamShell, WireComposerStreamsArgs};
use super::scroll::{wire_focus_message_after_nav, wire_messages_auto_scroll};

/// 注册 `wire_chat_domain_effects` 所需的信号与句柄（避免长形参列表）。
pub(crate) struct WireChatDomainEffectsArgs {
    pub initialized: RwSignal<bool>,
    pub chat_session: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub pending_images: RwSignal<Vec<String>>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub composer_draft_buffer: Arc<Mutex<String>>,
    pub composer_mirror_html: RwSignal<String>,
    pub composer_mirror_scroll_top: RwSignal<f64>,
    pub composer_input_ref: NodeRef<Textarea>,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub messages_scroller: NodeRef<leptos::html::Div>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub locale: RwSignal<Locale>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub focus_message_id_after_nav: RwSignal<Option<String>>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub stream_shell: ComposerStreamShell,
}

/// 注册聊天列与输入/流式相关 `wire_*`，并返回 composer 侧闭包（`new_session` / `cancel_stream` 等）。
pub(crate) fn wire_chat_domain_effects(args: WireChatDomainEffectsArgs) -> ChatComposerWires {
    let WireChatDomainEffectsArgs {
        initialized,
        chat_session,
        draft,
        pending_images,
        pending_clarification,
        collapsed_long_assistant_ids,
        composer_draft_buffer,
        composer_mirror_html,
        composer_mirror_scroll_top,
        composer_input_ref,
        sessions,
        active_id,
        messages_scroller,
        auto_scroll_chat,
        messages_scroll_from_effect,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        locale,
        apply_assistant_display_filters,
        focus_message_id_after_nav,
        selected_agent_role,
        stream_shell,
    } = args;

    wire_session_switch_clears_chat_state(
        initialized,
        chat_session,
        draft,
        pending_images,
        pending_clarification,
        collapsed_long_assistant_ids,
    );

    wire_draft_sync_to_buffer_and_textarea(
        draft,
        Arc::clone(&composer_draft_buffer),
        composer_input_ref.clone(),
        composer_mirror_html,
        composer_mirror_scroll_top,
    );

    wire_messages_auto_scroll(
        sessions,
        active_id,
        messages_scroller,
        auto_scroll_chat,
        messages_scroll_from_effect,
    );

    wire_chat_find_matches(
        sessions,
        active_id,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        auto_scroll_chat,
        locale,
        apply_assistant_display_filters,
    );

    wire_focus_message_after_nav(focus_message_id_after_nav);

    wire_chat_composer_streams(WireComposerStreamsArgs {
        initialized,
        chat: chat_session,
        locale,
        draft,
        selected_agent_role,
        stream_shell,
        composer_draft_buffer: Arc::clone(&composer_draft_buffer),
        auto_scroll_chat,
        pending_images,
    })
}
