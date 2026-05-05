//! 命令审批条（SSE 控制面 `run_command` 等）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::submit_chat_approval;
use crate::i18n::{self, Locale};

/// `pending_approval`: `(approval_session_id, command, args)`
#[component]
pub fn ApprovalBar(
    pending_approval: RwSignal<Option<(String, String, String)>>,
    approval_expanded: RwSignal<bool>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        {move || {
            pending_approval.get().map(|(sid, cmd, args)| {
                let sid_deny = sid.clone();
                let sid_once = sid.clone();
                let preview = format!("{cmd} {args}");
                let preview_short: String = preview.chars().take(72).collect();
                let preview_tail = if preview.chars().count() > 72 {
                    i18n::ellipsis_tail()
                } else {
                    ""
                };
                view! {
                    <div class="approval-bar">
                        <button
                            type="button"
                            class="approval-bar-toggle"
                            aria-expanded=move || approval_expanded.get()
                            on:click=move |_| approval_expanded.update(|e| *e = !*e)
                        >
                            <span class="approval-bar-toggle-label">
                                {move || i18n::approval_toggle_label(locale.get())}
                            </span>
                            <span class="approval-bar-toggle-preview">{preview_short}{preview_tail}</span>
                            <span class="approval-bar-chevron" aria-hidden="true">"▾"</span>
                        </button>
                        <div class=move || {
                            if approval_expanded.get() {
                                "approval-bar-detail"
                            } else {
                                "approval-bar-detail approval-bar-detail-collapsed"
                            }
                        }>
                            <pre>{cmd}" "{args}</pre>
                        </div>
                        <div class="actions">
                            <button type="button" class="btn btn-danger btn-sm" on:click={
                                let sid = sid_deny;
                                let loc = locale.get_untracked();
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "deny", loc).await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>
                                {move || i18n::approval_deny(locale.get())}
                            </button>
                            <button type="button" class="btn btn-secondary btn-sm" on:click={
                                let sid = sid_once.clone();
                                let loc = locale.get_untracked();
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "allow_once", loc).await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>
                                {move || i18n::approval_allow_once(locale.get())}
                            </button>
                            <button type="button" class="btn btn-primary btn-sm" on:click={
                                let sid = sid.clone();
                                let loc = locale.get_untracked();
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "allow_always", loc).await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>
                                {move || i18n::approval_allow_always(locale.get())}
                            </button>
                        </div>
                    </div>
                }
            })
        }}
    }
}
