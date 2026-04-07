//! 工作区变更预览模态（视图 + fetch / innerHTML 副作用）。

use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;

use crate::api::fetch_workspace_changelog;
use crate::markdown;

/// `changelist_fetch_nonce` 递增后拉取 `GET /workspace/changelog`；`nonce==0` 时不请求。
pub(super) fn wire_changelist_fetch_effects(
    conversation_id: RwSignal<Option<String>>,
    changelist_fetch_nonce: RwSignal<u64>,
    changelist_modal_loading: RwSignal<bool>,
    changelist_modal_err: RwSignal<Option<String>>,
    changelist_modal_html: RwSignal<String>,
    changelist_modal_rev: RwSignal<u64>,
) {
    Effect::new({
        let conversation_id = conversation_id;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        let changelist_modal_loading = changelist_modal_loading;
        let changelist_modal_err = changelist_modal_err;
        let changelist_modal_html = changelist_modal_html;
        let changelist_modal_rev = changelist_modal_rev;
        move |_| {
            let n = changelist_fetch_nonce.get();
            if n == 0 {
                return;
            }
            changelist_modal_loading.set(true);
            changelist_modal_err.set(None);
            let cid = conversation_id.get();
            spawn_local(async move {
                match fetch_workspace_changelog(cid.as_deref()).await {
                    Ok(r) => {
                        if let Some(e) = r.error {
                            changelist_modal_err.set(Some(e));
                            changelist_modal_html.set(String::new());
                            changelist_modal_rev.set(0);
                        } else {
                            changelist_modal_rev.set(r.revision);
                            changelist_modal_html.set(markdown::to_safe_html(&r.markdown));
                        }
                    }
                    Err(e) => {
                        changelist_modal_err.set(Some(e));
                        changelist_modal_html.set(String::new());
                        changelist_modal_rev.set(0);
                    }
                }
                changelist_modal_loading.set(false);
            });
        }
    });
}

/// 将渲染后的 HTML 写入模态正文容器（DOM 就绪后一帧再写）。
pub(super) fn wire_changelist_body_inner_html(
    changelist_modal_html: RwSignal<String>,
    changelist_body_ref: NodeRef<Div>,
) {
    Effect::new({
        let changelist_modal_html = changelist_modal_html;
        let changelist_body_ref = changelist_body_ref.clone();
        move |_| {
            let html = changelist_modal_html.get();
            let r = changelist_body_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(n) = r.get()
                    && let Ok(he) = n.dyn_into::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });
}

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
