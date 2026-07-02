//! Tauri 桌面：无边框时提供最小化 / 最大化 / 关闭。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::tauri_shell::{
    tauri_main_window_close, tauri_main_window_minimize, tauri_main_window_toggle_maximize,
    tauri_shell_available,
};

#[component]
pub fn TauriWindowControls(locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <Show when=move || tauri_shell_available()>
            <div
                class="tauri-window-controls"
                role="group"
                data-testid="tauri-window-controls"
                prop:aria-label=move || i18n::ide_tauri_window_controls_aria(locale.get())
            >
                <button
                    type="button"
                    class="tauri-win-ctrl tauri-win-ctrl-minimize"
                    prop:aria-label=move || i18n::ide_tauri_window_minimize(locale.get())
                    on:click=move |_| tauri_main_window_minimize()
                >
                    <span class="tauri-win-ctrl-glyph" aria-hidden="true">"−"</span>
                </button>
                <button
                    type="button"
                    class="tauri-win-ctrl tauri-win-ctrl-maximize"
                    prop:aria-label=move || i18n::ide_tauri_window_toggle_maximize(locale.get())
                    on:click=move |_| tauri_main_window_toggle_maximize()
                >
                    <span class="tauri-win-ctrl-glyph" aria-hidden="true">"□"</span>
                </button>
                <button
                    type="button"
                    class="tauri-win-ctrl tauri-win-ctrl-close"
                    prop:aria-label=move || i18n::ide_tauri_window_close(locale.get())
                    on:click=move |_| tauri_main_window_close()
                >
                    <span class="tauri-win-ctrl-glyph" aria-hidden="true">"×"</span>
                </button>
            </div>
        </Show>
    }
}
