//! IDE 文本编辑区（CodeMirror 6）。

use leptos::prelude::*;

use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::{self, Locale};
use crate::ide_codemirror::{IdeCmWireSignals, IdeEditorHost, wire_ide_codemirror};

#[component]
pub fn IdeEditorPane(
    locale: RwSignal<Locale>,
    editor: IdeEditorSignals,
    host: IdeEditorHost,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
) -> impl IntoView {
    wire_ide_codemirror(
        host,
        IdeCmWireSignals {
            ide_path,
            ide_text,
            ide_load_busy,
            line_numbers: editor.line_numbers,
            word_wrap: editor.word_wrap,
            tab_size: editor.tab_size,
            font_slug: editor.font_slug,
            font_size_px: editor.font_size_px,
        },
    );

    view! {
        <div
            id="ide-editor-panel"
            class="ide-editor-pane"
            class:ide-editor-pane--wrap=move || editor.word_wrap.get()
        >
            <Show when=move || !IdeEditorHost::cm_available()>
                <p class="ide-editor-cm-missing" role="alert">
                    {move || i18n::ide_cm_missing(locale.get())}
                </p>
            </Show>
            <div
                class="ide-cm-host"
                class:ide-cm-host--disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                prop:aria-label=move || {
                    if ide_path.get().is_none() {
                        i18n::ide_no_file(locale.get()).to_string()
                    } else {
                        String::new()
                    }
                }
                node_ref=host.container
                data-testid="ide-editor-cm"
            ></div>
        </div>
    }
}
