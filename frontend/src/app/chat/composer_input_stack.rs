//! 带工作区 `@引用` 镜像高亮的输入栈（底层 HTML + 透明文字 textarea）。

use std::sync::Arc;

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};

#[component]
pub fn ComposerInputStack(
    composer_input_ref: NodeRef<Textarea>,
    draft: RwSignal<String>,
    composer_mirror_html: RwSignal<String>,
    composer_mirror_scroll_top: RwSignal<f64>,
    run_send_message: Arc<dyn Fn() + Send + Sync>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    let mirror_inner_ref = NodeRef::<leptos::html::Div>::new();
    Effect::new({
        let mirror_inner_ref = mirror_inner_ref.clone();
        let composer_mirror_html = composer_mirror_html;
        move |_| {
            let h = composer_mirror_html.get();
            if let Some(el) = mirror_inner_ref.get() {
                el.set_inner_html(&h);
            }
        }
    });

    view! {
        <div class="composer-input-stack">
            <div class="composer-input-highlight" aria-hidden="true">
                <div
                    class="composer-input-highlight-inner"
                    node_ref=mirror_inner_ref
                    prop:style=move || {
                        format!("transform: translateY(-{}px)", composer_mirror_scroll_top.get())
                    }
                ></div>
            </div>
            <textarea
                class="composer-input composer-input--mirror-overlay"
                data-testid="chat-composer-input"
                node_ref=composer_input_ref
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    draft.set(v);
                }
                on:keydown={
                    let r = Arc::clone(&run_send_message);
                    move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() {
                            ev.prevent_default();
                            r();
                        }
                    }
                }
                on:scroll=move |ev: web_sys::Event| {
                    let Some(t) = ev.target() else {
                        return;
                    };
                    let Ok(ta) = t.dyn_into::<web_sys::HtmlTextAreaElement>() else {
                        return;
                    };
                    composer_mirror_scroll_top.set(ta.scroll_top() as f64);
                }
                prop:placeholder=move || i18n::composer_ph(locale.get())
                rows="3"
            ></textarea>
        </div>
    }
}
