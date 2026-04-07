//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复可折叠。

use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::markdown;
use crate::message_format::message_text_for_display;
use crate::storage::ChatSession;

/// 超过该字符数（按展示用 `message_text_for_display` 计）的已完成助手消息默认折叠。
const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

#[derive(Default)]
struct AssistantMdPaint {
    latest_html: String,
    raf_scheduled: bool,
    /// 本帧内是否曾出现「由空到有字」的流式首包（用于一次性淡入 class）。
    pending_first_chunk_anim: bool,
}

/// 助手非工具消息：Markdown → 净化 HTML；可选折叠长文。
pub fn assistant_markdown_collapsible_view(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    let body_ref = NodeRef::<Div>::new();
    let mid = message_id.clone();
    let mid_for_btn = message_id.clone();
    let prev_raw = StoredValue::new(Arc::new(Mutex::new(String::new())));
    let paint = StoredValue::new(Arc::new(Mutex::new(AssistantMdPaint::default())));

    Effect::new({
        let body_ref = body_ref.clone();
        let mid = mid.clone();
        move |_| {
            let _ = sessions.get();
            let _ = active_id.get();
            let _ = expanded_long_assistant_ids.get();
            let _ = locale.get();
            let loc = locale.get_untracked();
            let raw = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| message_text_for_display(m, loc))
                    .unwrap_or_default()
            });
            let is_loading = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| m.state.as_deref() == Some("loading"))
                    .unwrap_or(false)
            });
            let first_stream_chunk = {
                let arc = prev_raw.get_value();
                let mut g = arc.lock().expect("assistant prev_raw mutex poisoned");
                let prev_empty = g.is_empty();
                let first = prev_empty && !raw.is_empty() && is_loading;
                *g = raw.clone();
                first
            };
            let html = markdown::to_safe_html(&raw);
            let paint_arc = paint.get_value();
            {
                let mut g = paint_arc.lock().expect("assistant paint mutex poisoned");
                g.latest_html = html;
                if first_stream_chunk {
                    g.pending_first_chunk_anim = true;
                }
                if g.raf_scheduled {
                    return;
                }
                g.raf_scheduled = true;
            }
            let paint_run = Arc::clone(&paint_arc);
            let body_ref = body_ref.clone();
            request_animation_frame(move || {
                let (html, do_first) = {
                    let mut g = paint_run.lock().expect("assistant paint mutex poisoned");
                    g.raf_scheduled = false;
                    let html = g.latest_html.clone();
                    let do_first = g.pending_first_chunk_anim;
                    g.pending_first_chunk_anim = false;
                    (html, do_first)
                };
                if let Some(n) = body_ref.get()
                    && let Some(he) = n.dyn_ref::<web_sys::HtmlElement>()
                {
                    let _ = he.class_list().remove_1("msg-md-first-chunk");
                    he.set_inner_html(&html);
                    if do_first {
                        let _ = he.class_list().add_1("msg-md-first-chunk");
                    }
                }
            });
        }
    });

    view! {
        <div class="msg-md-wrap">
            {move || {
                let loc = locale.get();
                let (is_loading, raw_len) = sessions.with(|list| {
                    let aid = active_id.get();
                    let m = list
                        .iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| s.messages.iter().find(|msg| msg.id == mid_for_btn));
                    match m {
                        Some(msg) => (
                            msg.state.as_deref() == Some("loading"),
                            message_text_for_display(msg, loc).chars().count(),
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
                                {move || {
                                    if expanded {
                                        i18n::assistant_md_collapse(loc)
                                    } else {
                                        i18n::assistant_md_expand_full(loc)
                                    }
                                }}
                            </button>
                        }
                    })}
                }
            }}
        </div>
    }
}
