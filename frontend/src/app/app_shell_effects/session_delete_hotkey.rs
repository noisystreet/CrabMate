//! 全局会话删除快捷键（不在输入框内时），基于当前 UI 选中会话 [`ChatSessionSignals::active_id`]：
//! - **`Delete`**：确认后删除（与右键菜单一致）
//! - **`Shift` + `Delete`**：直接删除，无确认框

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;

use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::{delete_session_after_confirm, delete_session_immediate};

use super::escape::keyboard_event_target_is_text_entry;

#[derive(Clone, Copy)]
pub struct SessionDeleteHotkeySignals {
    pub chat: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub locale: RwSignal<crate::i18n::Locale>,
    pub session_modal: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
}

fn resolved_active_session_id(chat: ChatSessionSignals) -> Option<String> {
    let sid = chat.active_id.get_untracked();
    if sid.is_empty() {
        return None;
    }
    chat.sessions
        .with(|list| list.iter().any(|s| s.id == sid))
        .then_some(sid)
}

/// 焦点**不在**可编辑控件上时：**`Delete`** 确认删除；**`Shift`+`Delete`** 立即删除当前选中会话。
pub fn wire_session_delete_hotkey(signals: SessionDeleteHotkeySignals) {
    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.key() != "Delete" {
                return;
            }
            if keyboard_event_target_is_text_entry(&ev) {
                return;
            }
            if signals.session_modal.get_untracked()
                || signals.settings_modal.get_untracked()
                || signals.changelist_modal_open.get_untracked()
            {
                return;
            }
            let Some(sid) = resolved_active_session_id(signals.chat) else {
                return;
            };
            ev.prevent_default();
            ev.stop_propagation();
            if ev.shift_key() {
                delete_session_immediate(
                    signals.chat.sessions,
                    signals.chat.active_id,
                    signals.draft,
                    signals.chat.session_sync,
                    &sid,
                    signals.locale.get_untracked(),
                );
            } else {
                delete_session_after_confirm(
                    signals.chat.sessions,
                    signals.chat.active_id,
                    signals.draft,
                    signals.chat.session_sync,
                    &sid,
                    signals.locale.get_untracked(),
                );
            }
        });
        on_cleanup(move || h.remove());
    });
}
