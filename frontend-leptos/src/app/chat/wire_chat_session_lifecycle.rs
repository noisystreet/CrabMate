//! 会话首启、Web UI 展示偏好、服务端水合、会话列表持久化、上下文用量估算（从 `app/mod.rs` 迁入，阶段 B）。
//!
//! 调用顺序与原先在 `App` 内一致：**首启会话 → Web UI 一次配置 → 水合 → 持久化 → 用量估算**。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::ChatSession;

use crate::app::app_shell_effects::{
    wire_context_used_estimate, wire_initial_sessions_from_storage, wire_persist_chat_sessions,
    wire_web_ui_config_once_after_init,
};
use crate::app::session_hydrate::wire_session_hydration;

/// 注册与「会话生命周期 + 展示偏好」相关的壳级 `wire_*`（不含纯主题/侧栏宽度等）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn wire_chat_session_lifecycle_effects(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    locale: RwSignal<Locale>,
    web_ui_config_loaded: RwSignal<bool>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
    chat_session: ChatSessionSignals,
    selected_agent_role: RwSignal<Option<String>>,
    context_used_estimate: RwSignal<usize>,
) {
    wire_initial_sessions_from_storage(initialized, sessions, active_id, draft, locale);
    wire_web_ui_config_once_after_init(
        initialized,
        web_ui_config_loaded,
        markdown_render,
        apply_assistant_display_filters,
    );

    wire_session_hydration(
        initialized,
        web_ui_config_loaded,
        chat_session,
        locale,
        selected_agent_role,
    );

    wire_persist_chat_sessions(initialized, sessions, active_id);
    wire_context_used_estimate(
        initialized,
        sessions,
        active_id,
        draft,
        context_used_estimate,
    );
}
