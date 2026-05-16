//! Tauri IDE 模式：无边框时提供最小化 / 最大化 / 关闭（替代系统标题栏按钮）。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::tauri_shell::{
    tauri_main_window_close, tauri_main_window_minimize, tauri_main_window_toggle_maximize,
    tauri_shell_available,
};

#[component]
pub fn IdeTauriWindowControls(locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <Show when=tauri_shell_available>
            <div
                class="ide-tauri-window-controls"
                role="group"
                prop:aria-label=move || i18n::ide_tauri_window_controls_aria(locale.get())
            >
                <button
                    type="button"
                    class="ide-win-ctrl ide-win-ctrl-minimize"
                    prop:aria-label=move || i18n::ide_tauri_window_minimize(locale.get())
                    on:click=move |_| tauri_main_window_minimize()
                >
                    <span class="ide-win-ctrl-glyph" aria-hidden="true">"−"</span>
                </button>
                <button
                    type="button"
                    class="ide-win-ctrl ide-win-ctrl-maximize"
                    prop:aria-label=move || i18n::ide_tauri_window_toggle_maximize(locale.get())
                    on:click=move |_| tauri_main_window_toggle_maximize()
                >
                    <span class="ide-win-ctrl-glyph" aria-hidden="true">"□"</span>
                </button>
                <button
                    type="button"
                    class="ide-win-ctrl ide-win-ctrl-close"
                    prop:aria-label=move || i18n::ide_tauri_window_close(locale.get())
                    on:click=move |_| tauri_main_window_close()
                >
                    <span class="ide-win-ctrl-glyph" aria-hidden="true">"×"</span>
                </button>
            </div>
        </Show>
    }
}
