//! 工作区变更预览模态。

use leptos::html::Div;
use leptos::prelude::*;

pub fn changelist_modal_view(
    changelist_modal_open: RwSignal<bool>,
    changelist_modal_loading: RwSignal<bool>,
    changelist_modal_err: RwSignal<Option<String>>,
    changelist_modal_rev: RwSignal<u64>,
    changelist_fetch_nonce: RwSignal<u64>,
    changelist_body_ref: NodeRef<Div>,
) -> impl IntoView {
    view! {
            <Show when=move || changelist_modal_open.get()>
                <div class="changelist-modal-layer">
                    <div
                        class="changelist-modal-backdrop"
                        aria-hidden="true"
                        on:click=move |_| changelist_modal_open.set(false)
                    ></div>
                    <div
                        class="changelist-modal"
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="changelist-modal-title"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    >
                        <div class="changelist-modal-head">
                            <h2 id="changelist-modal-title" class="changelist-modal-title">
                                "会话工作区变更"
                            </h2>
                            <span class="changelist-modal-rev">{move || {
                                if changelist_modal_rev.get() > 0 {
                                    format!("rev {}", changelist_modal_rev.get())
                                } else {
                                    String::new()
                                }
                            }}</span>
                            <div class="changelist-modal-actions">
                                <button
                                    type="button"
                                    class="btn btn-secondary btn-sm"
                                    prop:disabled=move || changelist_modal_loading.get()
                                    on:click=move |_| {
                                        changelist_fetch_nonce.update(|x| *x = x.wrapping_add(1));
                                    }
                                >
                                    "刷新"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn-muted btn-sm"
                                    on:click=move |_| changelist_modal_open.set(false)
                                >
                                    "关闭"
                                </button>
                            </div>
                        </div>
                        <div class="changelist-modal-body">
                            <Show when=move || changelist_modal_loading.get()>
                                <p class="changelist-modal-status">"加载中…"</p>
                            </Show>
                            <Show when=move || changelist_modal_err.get().is_some()>
                                <p class="msg-error">{move || {
                                    changelist_modal_err.get().unwrap_or_default()
                                }}</p>
                            </Show>
                            <div
                                class="changelist-modal-prose msg-md-prose"
                                node_ref=changelist_body_ref
                            ></div>
                        </div>
                    </div>
                </div>
            </Show>
    }
}
