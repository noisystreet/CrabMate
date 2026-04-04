//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::markdown;
use crate::message_format::message_text_for_display;
use crate::storage::ChatSession;

/// 助手非工具消息：Markdown → 净化 HTML，流式更新时随 `sessions` 刷新。
pub fn assistant_markdown_body_view(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
) -> impl IntoView {
    let body_ref = NodeRef::<Div>::new();
    let mid = message_id;
    Effect::new(move |_| {
        let _ = sessions.get();
        let _ = active_id.get();
        let raw = sessions.with(|list| {
            let aid = active_id.get_untracked();
            list.iter()
                .find(|s| s.id == aid)
                .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                .map(message_text_for_display)
                .unwrap_or_default()
        });
        let html = markdown::to_safe_html(&raw);
        let r = body_ref.clone();
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(n) = r.get() {
                if let Some(he) = n.dyn_ref::<web_sys::HtmlElement>() {
                    he.set_inner_html(&html);
                }
            }
        });
    });
    view! {
        <div class="msg-body msg-md-prose" node_ref=body_ref></div>
    }
}
