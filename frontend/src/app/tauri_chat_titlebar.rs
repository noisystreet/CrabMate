//! Tauri 会话模式：窗口右上角最小化 / 最大化 / 关闭。

use leptos::prelude::*;

use crate::app::tauri_window_controls::TauriWindowControls;
use crate::i18n::Locale;
use crate::tauri_shell::tauri_shell_available;

#[component]
pub fn TauriChatTitlebar(
    locale: RwSignal<Locale>,
    editor_layout_mode: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <Show when=move || tauri_shell_available() && !editor_layout_mode.get()>
            <div class="tauri-chat-titlebar" data-testid="tauri-chat-titlebar">
                <TauriWindowControls locale=locale />
            </div>
        </Show>
    }
}
