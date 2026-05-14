//! 会话首启、Web UI 展示偏好、服务端水合、会话列表持久化（从 `app/mod.rs` 迁入，阶段 B）。
//!
//! 调用顺序与原先在 `App` 内一致：**首启会话 → Web UI 一次配置 → 水合 → 持久化**。与 [`crate::app::app_bootstrap_phase::AppBootstrapPhase`] 对应的门闸见 [`crate::app::session_hydrate::wire_session_hydration`] 等处的 `derive` 检查。

use leptos::prelude::*;

use crate::app::app_shell_effects::{
    wire_initial_sessions_from_storage, wire_persist_chat_sessions,
    wire_web_ui_config_once_after_init,
};
use crate::app::session_hydrate::wire_session_hydration;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::ChatSession;

/// `wire_chat_session_lifecycle_effects` 的输入（聚合壳级 RwSignal，避免长参数列表）。
pub(crate) struct WireChatSessionLifecycleEffectsArgs {
    pub initialized: RwSignal<bool>,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub draft: RwSignal<String>,
    pub locale: RwSignal<Locale>,
    pub web_ui_config_loaded: RwSignal<bool>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub chat_session: ChatSessionSignals,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub session_sessions_storage_key: RwSignal<String>,
}

impl WireChatSessionLifecycleEffectsArgs {
    /// 从 [`crate::app::app_signals::AppSignals`] 单点组装；`sessions` / `active_id` 与 [`ChatSessionSignals`] 同源字段一致。
    #[must_use]
    pub fn from_app_signals(app: &crate::app::app_signals::AppSignals) -> Self {
        Self {
            initialized: app.initialized,
            sessions: app.chat.sessions,
            active_id: app.chat.active_id,
            draft: app.chat_composer.draft,
            locale: app.shell_ui.locale,
            web_ui_config_loaded: app.shell_ui.web_ui_config_loaded,
            markdown_render: app.shell_ui.markdown_render,
            apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
            chat_session: app.chat,
            selected_agent_role: app.llm_settings.selected_agent_role,
            session_sessions_storage_key: app.session_sessions_storage_key,
        }
    }
}

/// 注册与「会话生命周期 + 展示偏好」相关的壳级 `wire_*`（不含纯主题/侧栏宽度等）。
pub(crate) fn wire_chat_session_lifecycle_effects(args: WireChatSessionLifecycleEffectsArgs) {
    let WireChatSessionLifecycleEffectsArgs {
        initialized,
        sessions,
        active_id,
        draft,
        locale,
        web_ui_config_loaded,
        markdown_render,
        apply_assistant_display_filters,
        chat_session,
        selected_agent_role,
        session_sessions_storage_key,
    } = args;

    wire_initial_sessions_from_storage(initialized, sessions, active_id, draft, locale);
    wire_web_ui_config_once_after_init(
        initialized,
        web_ui_config_loaded,
        markdown_render,
        apply_assistant_display_filters,
        locale,
    );

    wire_session_hydration(
        initialized,
        web_ui_config_loaded,
        chat_session,
        locale,
        selected_agent_role,
    );

    wire_persist_chat_sessions(initialized, chat_session, session_sessions_storage_key);
}
