//! 壳层通用确认对话框（会话删除等；替代 WebView 中无效的 `window.confirm`）。

use leptos::prelude::*;

use crate::app::app_signals::ModalSignals;
use crate::i18n::{self, Locale};
use crate::ide_confirm::resolve_ide_confirm;

#[component]
pub fn ShellConfirmDialog(locale: RwSignal<Locale>, modal: ModalSignals) -> impl IntoView {
    let confirm = modal.confirm_signals();
    view! {
        <Show when=move || modal.confirm_pending.get().is_some()>
            <div
                class="modal-backdrop shell-confirm-backdrop"
                data-testid="shell-confirm-dialog"
            >
                <div
                    class="modal ide-confirm-modal"
                    role="alertdialog"
                    aria-modal="true"
                    aria-labelledby="shell-confirm-title"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                >
                    <div class="modal-head">
                        <span id="shell-confirm-title" class="modal-title">
                            {move || i18n::ide_confirm_title(locale.get())}
                        </span>
                    </div>
                    <div class="modal-body">
                        <p class="modal-hint">
                            {move || {
                                modal
                                    .confirm_pending
                                    .get()
                                    .map(|p| p.message)
                                    .unwrap_or_default()
                            }}
                        </p>
                    </div>
                    <div class="modal-footer actions">
                        <button
                            type="button"
                            class="btn btn-secondary"
                            data-testid="shell-confirm-cancel"
                            on:click=move |_| resolve_ide_confirm(confirm, false)
                        >
                            {move || i18n::ide_confirm_cancel(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-danger"
                            data-testid="shell-confirm-ok"
                            on:click=move |_| resolve_ide_confirm(confirm, true)
                        >
                            {move || i18n::ide_confirm_ok(locale.get())}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
