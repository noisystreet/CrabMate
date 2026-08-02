//! 底栏 Ask / Plan / Act 三段切换（与 `agent_role` 正交）。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_ops::{make_message_id, message_created_ms, patch_active_session};
use crate::storage::StoredMessage;

const MODES: [(&str, &str); 3] = [("ask", "Ask"), ("plan", "Plan"), ("act", "Act")];

#[derive(Clone, Copy)]
pub struct SessionModeSegProps {
    pub locale: RwSignal<Locale>,
    pub chat: ChatSessionSignals,
    pub selected_session_mode: RwSignal<String>,
    pub session_mode_user_override: RwSignal<bool>,
}

fn apply_session_mode_selection(
    chat: ChatSessionSignals,
    selected_session_mode: RwSignal<String>,
    session_mode_user_override: RwSignal<bool>,
    locale: Locale,
    mode: &str,
) {
    let mode = mode.trim().to_ascii_lowercase();
    if !matches!(mode.as_str(), "ask" | "plan" | "act") {
        return;
    }
    if selected_session_mode.get_untracked() == mode {
        session_mode_user_override.set(true);
        return;
    }
    selected_session_mode.set(mode.clone());
    session_mode_user_override.set(true);
    chat.clear_stream_resume_handles();
    let notice = i18n::status_session_mode_switched(locale, mode.as_str());
    let mid = make_message_id();
    let now = message_created_ms();
    patch_active_session(chat.sessions, &chat.active_id.get_untracked(), |s| {
        s.messages.push(StoredMessage {
            id: mid,
            role: "system".into(),
            text: notice,
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
    });
}

#[component]
pub fn StatusSessionModeSeg(props: SessionModeSegProps) -> impl IntoView {
    let SessionModeSegProps {
        locale,
        chat,
        selected_session_mode,
        session_mode_user_override,
    } = props;
    view! {
        <div
            class="status-session-mode-seg"
            role="group"
            prop:aria-label=move || i18n::status_mode_label(locale.get())
        >
            {MODES
                .into_iter()
                .map(|(id, short)| {
                    let id_owned = id.to_string();
                    let id_for_active = id_owned.clone();
                    let id_for_click = id_owned.clone();
                    view! {
                        <button
                            type="button"
                            class="status-session-mode-btn"
                            class:active=move || selected_session_mode.get() == id_for_active
                            prop:title=move || {
                                i18n::status_session_mode_title(locale.get(), id)
                            }
                            on:click=move |_| {
                                apply_session_mode_selection(
                                    chat,
                                    selected_session_mode,
                                    session_mode_user_override,
                                    locale.get_untracked(),
                                    id_for_click.as_str(),
                                );
                            }
                        >
                            {short}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}
