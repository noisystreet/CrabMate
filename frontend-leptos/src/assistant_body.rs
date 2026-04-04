//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复可折叠。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::markdown;
use crate::message_format::message_text_for_display;
use crate::storage::ChatSession;

/// 超过该字符数（按展示用 `message_text_for_display` 计）的已完成助手消息默认折叠。
const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

/// 助手非工具消息：Markdown → 净化 HTML；可选折叠长文。
pub fn assistant_markdown_collapsible_view(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
) -> impl IntoView {
    let body_ref = NodeRef::<Div>::new();
    let mid = message_id.clone();
    let mid_for_btn = message_id.clone();

    Effect::new({
        let body_ref = body_ref.clone();
        let mid = mid.clone();
        move |_| {
            let _ = sessions.get();
            let _ = active_id.get();
            let _ = expanded_long_assistant_ids.get();
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
                if let Some(n) = r.get()
                    && let Some(he) = n.dyn_ref::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });

    view! {
        <div class="msg-md-wrap">
            {move || {
                let (is_loading, raw_len) = sessions.with(|list| {
                    let aid = active_id.get();
                    let m = list
                        .iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| s.messages.iter().find(|msg| msg.id == mid_for_btn));
                    match m {
                        Some(msg) => (
                            msg.state.as_deref() == Some("loading"),
                            message_text_for_display(msg).chars().count(),
                        ),
                        None => (false, 0),
                    }
                });
                let long = !is_loading && raw_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD;
                let expanded =
                    expanded_long_assistant_ids.with(|v| v.iter().any(|id| id == &mid_for_btn));
                let collapsed = long && !expanded;
                let wrap_cls = if collapsed {
                    "msg-body msg-md-prose msg-md-prose-collapsed"
                } else {
                    "msg-body msg-md-prose"
                };
                let btn_mid = mid_for_btn.clone();
                let exp_sig = expanded_long_assistant_ids;
                view! {
                    <div class=wrap_cls node_ref=body_ref></div>
                    {long.then(move || {
                        let b = btn_mid.clone();
                        view! {
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-md-toggle"
                                on:click=move |_| {
                                    exp_sig.update(|v| {
                                        if v.iter().any(|id| id == &b) {
                                            v.retain(|id| id != &b);
                                        } else {
                                            v.push(b.clone());
                                        }
                                    });
                                }
                            >
                                {if expanded { "收起" } else { "展开全文" }}
                            </button>
                        }
                    })}
                }
            }}
        </div>
    }
}
