//! 工作区变更预览模态（视图 + fetch / innerHTML 副作用）。
//!
//! 拉取结果用 [`ChangelistModalBodyState`] 显式状态机表达：`Idle` / `Loading`（保留上一轮 HTML 与 revision 以匹配旧行为）/ `Ready` / `Failed`。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::api::fetch_workspace_changelog;
use crate::i18n::{self, load_locale_from_storage};
use crate::message_render::fragment_to_chat_safe_html;
use crate::session_sync::SessionSyncState;

use super::app_shell_ctx::ChangelistModalSignals;

/// 变更集正文区：与 `GET /workspace/changelog` 请求生命周期一致的状态机。
#[derive(Clone, Debug, Default)]
pub enum ChangelistModalBodyState {
    #[default]
    Idle,
    /// 请求在途；`retain_*` 用于在加载动画期间继续展示上一轮成功内容（与旧 `loading==true` 且不清空 `html` 对齐）。
    Loading {
        retain_html: String,
        retain_rev: u64,
    },
    Ready {
        revision: u64,
        html: String,
    },
    Failed {
        message: String,
    },
}

impl ChangelistModalBodyState {
    fn begin_fetch_from(&self) -> Self {
        let (retain_html, retain_rev) = match self {
            ChangelistModalBodyState::Ready { revision, html } => (html.clone(), *revision),
            ChangelistModalBodyState::Loading {
                retain_html,
                retain_rev,
            } => (retain_html.clone(), *retain_rev),
            ChangelistModalBodyState::Failed { .. } | ChangelistModalBodyState::Idle => {
                (String::new(), 0)
            }
        };
        Self::Loading {
            retain_html,
            retain_rev,
        }
    }

    #[must_use]
    fn prose_html_for_dom(&self) -> String {
        match self {
            ChangelistModalBodyState::Loading { retain_html, .. } => retain_html.clone(),
            ChangelistModalBodyState::Ready { html, .. } => html.clone(),
            ChangelistModalBodyState::Failed { .. } | ChangelistModalBodyState::Idle => {
                String::new()
            }
        }
    }

    #[must_use]
    fn head_revision(&self) -> u64 {
        match self {
            ChangelistModalBodyState::Ready { revision, .. } => *revision,
            ChangelistModalBodyState::Loading { retain_rev, .. } => *retain_rev,
            ChangelistModalBodyState::Failed { .. } | ChangelistModalBodyState::Idle => 0,
        }
    }

    #[must_use]
    fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }

    #[must_use]
    fn failed_message(&self) -> Option<&str> {
        match self {
            ChangelistModalBodyState::Failed { message } => Some(message.as_str()),
            _ => None,
        }
    }
}

/// `changelist_fetch_nonce` 递增后拉取 `GET /workspace/changelog`；`nonce==0` 时不请求。
pub(super) fn wire_changelist_fetch_effects(
    session_sync: RwSignal<SessionSyncState>,
    changelist_fetch_nonce: RwSignal<u64>,
    changelist_modal_body: RwSignal<ChangelistModalBodyState>,
    markdown_render: RwSignal<bool>,
) {
    Effect::new({
        let session_sync = session_sync;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        let changelist_modal_body = changelist_modal_body;
        let markdown_render = markdown_render;
        move |_| {
            let n = changelist_fetch_nonce.get();
            if n == 0 {
                return;
            }
            changelist_modal_body.update(|st| *st = st.begin_fetch_from());
            let cid = session_sync.with(|s| s.changelog_conversation_id().map(str::to_string));
            let md_on = markdown_render.get_untracked();
            spawn_local(async move {
                let loc = load_locale_from_storage();
                match fetch_workspace_changelog(cid.as_deref(), loc).await {
                    Ok(r) => {
                        if let Some(e) = r.error {
                            changelist_modal_body
                                .set(ChangelistModalBodyState::Failed { message: e });
                        } else {
                            changelist_modal_body.set(ChangelistModalBodyState::Ready {
                                revision: r.revision,
                                html: fragment_to_chat_safe_html(&r.markdown, md_on),
                            });
                        }
                    }
                    Err(e) => {
                        changelist_modal_body.set(ChangelistModalBodyState::Failed { message: e });
                    }
                }
            });
        }
    });
}

/// 将渲染后的 HTML 写入模态正文容器（DOM 就绪后一帧再写）。
pub(super) fn wire_changelist_body_inner_html(
    changelist_modal_body: RwSignal<ChangelistModalBodyState>,
    changelist_body_ref: NodeRef<Div>,
) {
    Effect::new({
        let changelist_modal_body = changelist_modal_body;
        let changelist_body_ref = changelist_body_ref.clone();
        move |_| {
            let html = changelist_modal_body.get().prose_html_for_dom();
            let r = changelist_body_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(n) = r.get_untracked()
                    && let Ok(he) = n.dyn_into::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });
}

fn changelist_modal_head(
    locale: RwSignal<i18n::Locale>,
    changelist_modal_body: RwSignal<ChangelistModalBodyState>,
    changelist_fetch_nonce: RwSignal<u64>,
    changelist_modal_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="changelist-modal-head">
            <h2 id="changelist-modal-title" class="changelist-modal-title">
                {move || i18n::changelist_title(locale.get())}
            </h2>
            <span class="changelist-modal-rev">{move || {
                let n = changelist_modal_body.get().head_revision();
                if n > 0 {
                    i18n::changelist_rev(locale.get(), n)
                } else {
                    String::new()
                }
            }}</span>
            <div class="changelist-modal-actions">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:disabled=move || changelist_modal_body.get().is_loading()
                    on:click=move |_| {
                        changelist_fetch_nonce.update(|x| *x = x.wrapping_add(1));
                    }
                >
                    {move || i18n::changelist_refresh(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-muted btn-sm"
                    on:click=move |_| changelist_modal_open.set(false)
                >
                    {move || i18n::settings_close(locale.get())}
                </button>
            </div>
        </div>
    }
}

fn changelist_modal_body_section(
    locale: RwSignal<i18n::Locale>,
    body: RwSignal<ChangelistModalBodyState>,
    changelist_body_ref: NodeRef<Div>,
) -> impl IntoView {
    view! {
        <div class="changelist-modal-body">
            <Show when=move || body.get().is_loading()>
                <p class="changelist-modal-status">{move || i18n::changelist_loading(locale.get())}</p>
            </Show>
            <Show when=move || body.get().failed_message().is_some()>
                <p class="msg-error">{move || {
                    body.get().failed_message().unwrap_or("").to_string()
                }}</p>
            </Show>
            <div
                class="changelist-modal-prose msg-md-prose"
                node_ref=changelist_body_ref
            ></div>
        </div>
    }
}

pub fn changelist_modal_view(signals: ChangelistModalSignals) -> impl IntoView {
    let ChangelistModalSignals {
        changelist_modal_open,
        locale,
        changelist_modal_body,
        changelist_fetch_nonce,
        changelist_body_ref,
    } = signals;
    let dialog_ref = NodeRef::<Div>::new();

    Effect::new({
        let dialog_ref = dialog_ref.clone();
        let open = changelist_modal_open;
        move |_| {
            if !open.get() {
                return;
            }
            let r = dialog_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = r.get() {
                    focus_first_in_modal_container(el.as_ref());
                }
            });
        }
    });

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
                        node_ref=dialog_ref
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="changelist-modal-title"
                        tabindex="-1"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Tab" {
                                if let Some(el) = dialog_ref.get() {
                                    trap_tab_in_container(&ev, el.as_ref());
                                }
                            }
                        }
                    >
                        {changelist_modal_head(
                            locale,
                            changelist_modal_body,
                            changelist_fetch_nonce,
                            changelist_modal_open,
                        )}
                        {changelist_modal_body_section(
                            locale,
                            changelist_modal_body,
                            changelist_body_ref,
                        )}
                    </div>
                </div>
            </Show>
    }
}
