//! 命令审批弹窗（阻塞式，替代 ApprovalBar）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::submit_chat_approval;
use crate::i18n::{self, Locale};

/// 审批弹窗入参聚合（阶段 B：`App` 单行传入）。
#[derive(Clone, Copy)]
pub struct ApprovalModalSignals {
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub locale: RwSignal<Locale>,
}

/// `pending_approval`: `(approval_session_id, command, args)`
#[component]
pub fn ApprovalModal(signals: ApprovalModalSignals) -> impl IntoView {
    let ApprovalModalSignals {
        pending_approval,
        locale,
    } = signals;
    let deny = {
        let pending_approval = pending_approval.clone();
        let locale = locale.clone();
        move |_| {
            if let Some((sid, _, _)) = pending_approval.get() {
                let loc = locale.get_untracked();
                let sid = sid.clone();
                spawn_local(async move {
                    let _ = submit_chat_approval(&sid, "deny", loc).await;
                });
                pending_approval.set(None);
            }
        }
    };

    let allow_once = {
        let pending_approval = pending_approval.clone();
        let locale = locale.clone();
        move |_| {
            if let Some((_sid, _, _)) = pending_approval.get() {
                let loc = locale.get_untracked();
                let sid = _sid.clone();
                spawn_local(async move {
                    let _ = submit_chat_approval(&sid, "allow_once", loc).await;
                });
                pending_approval.set(None);
            }
        }
    };

    let allow_always = {
        let pending_approval = pending_approval.clone();
        let locale = locale.clone();
        move |_| {
            if let Some((_sid, _, _)) = pending_approval.get() {
                let loc = locale.get_untracked();
                let sid = _sid.clone();
                spawn_local(async move {
                    let _ = submit_chat_approval(&sid, "allow_always", loc).await;
                });
                pending_approval.set(None);
            }
        }
    };

    view! {
        <Show when=move || pending_approval.get().is_some()>
            <div class="modal-backdrop">
                <div
                    class="modal approval-modal"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="approval-modal-title"
                    data-testid="approval-modal"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                >
                    <div class="modal-head">
                        <span id="approval-modal-title" class="modal-title">
                            {"⚠️ "}
                            {move || i18n::approval_modal_title(locale.get())}
                        </span>
                    </div>

                    <div class="modal-body">
                        <p class="approval-modal-intro">
                            {move || i18n::approval_modal_intro(locale.get())}
                        </p>
                        {move || {
                            pending_approval.get().map(|(_sid, cmd, args)| {
                                let full = format!("{} {}", cmd, args);
                                view! {
                                    <pre class="approval-modal-command">{full}</pre>
                                }
                            })
                        }}
                    </div>

                    <div class="modal-footer actions approval-modal-actions">
                        <button
                            type="button"
                            class="btn btn-danger"
                            data-testid="approval-deny"
                            on:click=deny
                        >
                            {move || i18n::approval_deny(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-secondary"
                            data-testid="approval-allow-once"
                            on:click=allow_once
                        >
                            {move || i18n::approval_allow_once(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-testid="approval-allow-always"
                            on:click=allow_always
                        >
                            {move || i18n::approval_allow_always(locale.get())}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
