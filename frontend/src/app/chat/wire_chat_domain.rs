//! 聊天域接线：会话切换、草稿同步、滚底、查找、composer 流式等 `wire_*` 从 `app/mod.rs` 迁出，落实 **`docs/frontend/ARCHITECTURE.md`** 阶段 **B（壳与域分离）**。
//!
//! 会话列表与活动 id 仅经 [`ChatDomainWiringSignals::chat`]（[`ChatSessionSignals`]）传递，**不**在 [`WireChatDomainEffectsArgs`] 中重复平铺 `sessions` / `active_id`。
//!
//! `App` 仍负责 [`AppSignals`](crate::app::app_signals::AppSignals) 与 [`ComposerStreamShell::from_app_signals`](super::handles::ComposerStreamShell::from_app_signals)；本模块只注册聊天相关副作用，并返回 [`ChatComposerWires`](super::handles::ChatComposerWires) 与 [`ChatStreamBusyMemos`](crate::chat_session_state::ChatStreamBusyMemos)（`Memo` 派生的回合忙状态，供底栏 / 合成器 / 消息行共用）。
//!
//! # `wire_chat_domain_effects` 内顺序
//!
//! 1. [`wire_session_switch_clears_chat_state`](super::composer::wire_session_switch_clears_chat_state) — 切会话时加载草稿与 `session_sync`。  
//! 2. [`wire_draft_sync_to_mirror_and_textarea`](super::composer::wire_draft_sync_to_mirror_and_textarea) — `draft` → 镜像层与 textarea。  
//! 3. [`wire_content_follow_scroll`](super::scroll_follow::wire_content_follow_scroll)。  
//! 4. [`wire_chat_find_matches`](super::find::wire_chat_find_matches)。  
//! 5. [`wire_focus_message_after_nav`](super::scroll::wire_focus_message_after_nav)。  
//! 6. [`wire_chat_composer_streams`](super::composer::wire_chat_composer_streams)。  
//!
//! 调整顺序前须确认：会话切换与草稿同步应先于依赖 `draft` / `active_id` 的发送闭包稳定注册。

use crate::app::app_signals::ChatDomainWiringSignals;
use crate::chat_session_state::{ChatStreamBusyMemos, make_chat_stream_busy_memos};

use super::composer::{
    wire_chat_composer_streams, wire_draft_sync_to_mirror_and_textarea,
    wire_session_switch_clears_chat_state,
};
use super::find::wire_chat_find_matches;
use super::handles::{
    ChatComposerWires, ComposerStreamShell, WireComposerStreamsArgs,
    WireComposerStreamsSessionSlice, WireComposerStreamsStreamSlice,
};
use super::scroll::wire_focus_message_after_nav;
use super::scroll_follow::wire_content_follow_scroll;
use super::scroll_shell::ChatScrollShellSignals;

/// 注册 `wire_chat_domain_effects` 所需的信号与句柄：[`ChatDomainWiringSignals`] + 流式壳。
#[derive(Clone)]
pub(crate) struct WireChatDomainEffectsArgs {
    pub domain: ChatDomainWiringSignals,
    pub stream_shell: ComposerStreamShell,
}

impl WireChatDomainEffectsArgs {
    #[must_use]
    pub fn from_app_and_stream_shell(
        app: &crate::app::app_signals::AppSignals,
        stream_shell: ComposerStreamShell,
    ) -> Self {
        Self {
            domain: app.chat_domain_wiring(),
            stream_shell,
        }
    }
}

/// 会话切换、草稿、滚动、查找、焦点（须在 [`wire_chat_composer_streams`] 之前注册）。
fn wire_chat_domain_auxiliary_sequence(a: &WireChatDomainEffectsArgs) {
    let d = &a.domain;
    wire_session_switch_clears_chat_state(
        d.initialized,
        d.chat,
        d.composer.draft,
        d.composer.pending_images,
        d.pending_clarification,
        d.composer.collapsed_long_assistant_ids,
        d.composer.tool_detail_expanded_ids,
    );

    wire_draft_sync_to_mirror_and_textarea(
        d.composer.draft,
        d.composer.composer_input_ref.clone(),
        d.composer.composer_mirror_html,
        d.composer.composer_mirror_scroll_top,
    );

    let scroll_shell = ChatScrollShellSignals::from_composer(&d.composer);
    wire_content_follow_scroll(d.chat, scroll_shell);

    wire_chat_find_matches(
        d.chat,
        d.composer.chat_find_query,
        d.composer.chat_find_match_ids,
        d.composer.chat_find_cursor,
        d.composer.auto_scroll_chat,
        d.locale,
        d.apply_assistant_display_filters,
    );

    wire_focus_message_after_nav(d.composer.focus_message_id_after_nav);
}

/// 注册聊天列与输入/流式相关 `wire_*`，并返回 composer 侧闭包与 **`Memo` 派生的回合忙状态**（底栏 / 合成器 / 消息行共用）。
pub(crate) fn wire_chat_domain_effects(
    args: WireChatDomainEffectsArgs,
) -> (ChatComposerWires, ChatStreamBusyMemos) {
    let stream_shell = args.stream_shell.clone();
    let abort_cell = stream_shell.stream.abort_cell.clone();
    let stream_busy_memos = make_chat_stream_busy_memos(
        args.domain.chat,
        stream_shell.stream.status_busy,
        stream_shell.stream.tool_busy,
        stream_shell.stream.stream_abort_epoch,
        std::sync::Arc::new(move || abort_cell.lock().unwrap().is_some()),
    );

    wire_chat_domain_auxiliary_sequence(&args);

    let d = &args.domain;
    let wires = wire_chat_composer_streams(WireComposerStreamsArgs {
        session: WireComposerStreamsSessionSlice {
            initialized: d.initialized,
            chat: d.chat,
            locale: d.locale,
            draft: d.composer.draft,
            selected_agent_role: d.selected_agent_role,
            agent_role_user_override: d.agent_role_user_override,
        },
        stream: WireComposerStreamsStreamSlice {
            stream_shell: args.stream_shell,
            stream_turn_busy_ui: stream_busy_memos.stream_turn_busy_ui,
            scroll_shell: ChatScrollShellSignals::from_composer(&d.composer),
            pending_images: d.composer.pending_images,
        },
    });
    (wires, stream_busy_memos)
}
