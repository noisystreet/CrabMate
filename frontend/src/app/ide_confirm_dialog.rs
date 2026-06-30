//! IDE 未保存更改等确认对话框。

use leptos::prelude::*;

use crate::app::app_signals::IdeChromeSignals;
use crate::i18n::{self, Locale};
use crate::ide_confirm::resolve_ide_confirm;

#[component]
pub fn IdeConfirmDialog(locale: RwSignal<Locale>, chrome: IdeChromeSignals) -> impl IntoView {
    let confirm = chrome.confirm_signals();
    view! {
        <Show when=move || chrome.confirm_pending.get().is_some()>
            <div class="modal-backdrop" data-testid="ide-confirm-dialog">
                <div
                    class="modal ide-confirm-modal"
                    role="alertdialog"
                    aria-modal="true"
                    aria-labelledby="ide-confirm-title"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                >
                    <div class="modal-head">
                        <span id="ide-confirm-title" class="modal-title">
                            {move || i18n::ide_confirm_title(locale.get())}
                        </span>
                    </div>
                    <div class="modal-body">
                        <p class="modal-hint">
                            {move || {
                                chrome
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
                            data-testid="ide-confirm-cancel"
                            on:click=move |_| resolve_ide_confirm(confirm, false)
                        >
                            {move || i18n::ide_confirm_cancel(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-testid="ide-confirm-ok"
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
