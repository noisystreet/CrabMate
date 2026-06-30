//! IDE 文本编辑区：行号栏、多语言语法高亮镜像层（Rust / TOML / YAML / C / C++ / Python）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::app::app_signals::IdeEditorSignals;
use crate::i18n::{self, Locale};
use crate::ide_editor_prefs::ide_editor_font_family_css;
use crate::ide_syntax_highlight::{highlight_source_for_path, ide_path_has_syntax_highlight};

fn ide_line_gutter_text(text: &str) -> String {
    let n = text.lines().count().max(1);
    (1..=n)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[component]
pub fn IdeEditorPane(
    locale: RwSignal<Locale>,
    editor: IdeEditorSignals,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
    textarea_ref: NodeRef<leptos::html::Textarea>,
) -> impl IntoView {
    let gutter_ref = NodeRef::<leptos::html::Pre>::new();
    let highlight_code_ref = NodeRef::<leptos::html::Code>::new();
    let highlight_scroll_top = RwSignal::new(0.0);

    let gutter_text = Memo::new(move |_| ide_line_gutter_text(&ide_text.get()));

    let syntax_highlight = Memo::new(move |_| {
        let path = ide_path.get();
        highlight_source_for_path(path.as_deref(), &ide_text.get())
    });

    let has_syntax_highlight =
        Memo::new(move |_| ide_path_has_syntax_highlight(ide_path.get().as_deref()));

    Effect::new({
        let highlight_code_ref = highlight_code_ref.clone();
        let syntax_highlight = syntax_highlight;
        move |_| {
            let html = syntax_highlight.get();
            if let Some(el) = highlight_code_ref.get() {
                el.set_inner_html(&html);
            }
        }
    });

    let pane_style = Memo::new(move |_| {
        format!(
            "--ide-editor-font-family:{};--ide-editor-font-size:{}px;--ide-editor-tab-size:{};",
            ide_editor_font_family_css(&editor.font_slug.get()),
            editor.font_size_px.get().round(),
            editor.tab_size.get(),
        )
    });

    let sync_scroll = move |ev: web_sys::Event| {
        let Some(ta) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
        else {
            return;
        };
        let top = ta.scroll_top();
        highlight_scroll_top.set(top as f64);
        if let Some(gutter) = gutter_ref.get() {
            gutter.set_scroll_top(top);
        }
    };

    view! {
        <div
            id="ide-editor-panel"
            class="ide-editor-pane"
            class:ide-editor-pane--line-numbers=move || editor.line_numbers.get()
            class:ide-editor-pane--wrap=move || editor.word_wrap.get()
            style=move || pane_style.get()
        >
            <Show when=move || editor.line_numbers.get()>
                <pre
                    class="ide-line-gutter"
                    node_ref=gutter_ref
                    aria-hidden="true"
                >
                    {move || gutter_text.get()}
                </pre>
            </Show>
            <div class="ide-editor-code-col">
                <div
                    class="ide-editor-stack"
                    class:ide-editor-stack--highlight=move || has_syntax_highlight.get()
                >
                    <pre class="ide-editor-highlight" aria-hidden="true">
                        <code
                            node_ref=highlight_code_ref
                            prop:style=move || {
                                format!("transform: translateY(-{}px)", highlight_scroll_top.get())
                            }
                        ></code>
                    </pre>
                    <textarea
                        node_ref=textarea_ref
                        class="ide-editor-textarea"
                        prop:spellcheck="false"
                        prop:placeholder=move || {
                            if ide_path.get().is_none() {
                                i18n::ide_no_file(locale.get())
                            } else {
                                ""
                            }
                        }
                        prop:disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                        on:scroll=sync_scroll
                        bind:value=ide_text
                    />
                </div>
            </div>
        </div>
    }
}
