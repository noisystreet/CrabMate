//! 命令审批条（SSE 控制面 `run_command` 等）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::submit_chat_approval;

/// `pending_approval`: `(approval_session_id, command, args)`
#[component]
pub fn ApprovalBar(
    pending_approval: RwSignal<Option<(String, String, String)>>,
    approval_expanded: RwSignal<bool>,
) -> impl IntoView {
    view! {
        {move || {
            pending_approval.get().map(|(sid, cmd, args)| {
                let sid_deny = sid.clone();
                let sid_once = sid.clone();
                let preview = format!("{cmd} {args}");
                let preview_short: String = preview.chars().take(72).collect();
                let preview_tail = if preview.chars().count() > 72 {
                    "…"
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
                            <span class="approval-bar-toggle-label">"需要审批：运行命令"</span>
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
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "deny").await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>"拒绝"</button>
                            <button type="button" class="btn btn-secondary btn-sm" on:click={
                                let sid = sid_once.clone();
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "allow_once").await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>"允许一次"</button>
                            <button type="button" class="btn btn-primary btn-sm" on:click={
                                let sid = sid.clone();
                                move |_| {
                                    let s = sid.clone();
                                    spawn_local(async move {
                                        let _ = submit_chat_approval(&s, "allow_always").await;
                                        pending_approval.set(None);
                                    });
                                }
                            }>"始终允许"</button>
                        </div>
                    </div>
                }
            })
        }}
    }
}
