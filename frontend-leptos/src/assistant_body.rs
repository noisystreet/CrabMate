//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复默认全文，可由用户折叠。
//!
//! 展示链与 HTML 出口见 [`crate::message_render`]。

use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::message_format::message_text_for_display_ex;
use crate::message_render::fragment_to_chat_safe_html;
use crate::storage::ChatSession;

/// 超过该字符数的已完成助手消息可手动折叠（作用于整条消息，含思考区）。
const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

#[derive(Default)]
struct SectionPaint {
    latest_html: String,
    raf_scheduled: bool,
}

impl SectionPaint {
    fn take_html(&mut self) -> String {
        std::mem::take(&mut self.latest_html)
    }
}

/// 助手非工具消息：Markdown → 净化 HTML；思维链独立区域 + 终答区。
pub fn assistant_markdown_collapsible_view(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let answer_body_ref = NodeRef::<Div>::new();
    let mid = message_id.clone();
    let mid_for_btn = message_id.clone();

    let answer_paint = StoredValue::new(Arc::new(Mutex::new(SectionPaint::default())));

    // ---------- 回答区 Effect ----------
    Effect::new({
        let answer_body_ref = answer_body_ref.clone();
        let answer_paint = answer_paint.clone();
        let mid = mid.clone();
        move |_| {
            let _ = sessions.get();
            let _ = active_id.get();
            let _ = collapsed_long_assistant_ids.get();
            let _ = locale.get();
            let _ = markdown_render.get();
            let _ = apply_assistant_display_filters.get();
            let (text_src, is_loading_msg) = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| (m.text.clone(), m.state.as_deref() == Some("loading")))
                    .unwrap_or_default()
            });

            // 流式生成中先按纯文本渲染，避免半截 Markdown（尤其未闭合代码围栏）
            // 在不同浏览器里触发布局伪影（如黑条/闪动）；完成后自动切回 Markdown。
            let md_on = markdown_render.get_untracked() && !is_loading_msg;
            let html = fragment_to_chat_safe_html(&text_src, md_on);
            let paint_arc = answer_paint.get_value();
            {
                let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                if g.raf_scheduled {
                    return;
                }
                g.latest_html = html;
                g.raf_scheduled = true;
            }
            let paint_run = Arc::clone(&paint_arc);
            let answer_body_ref = answer_body_ref.clone();
            request_animation_frame(move || {
                let html = {
                    let mut g = paint_run.lock().expect("answer paint mutex poisoned");
                    g.raf_scheduled = false;
                    g.take_html()
                };
                if let Some(n) = answer_body_ref.get_untracked()
                    && let Some(he) = n.dyn_ref::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });

    let mid_stored = StoredValue::new(mid_for_btn.clone());

    view! {
        <div class=move || {
            if markdown_render.get() {
                "msg-md-wrap"
            } else {
                "msg-md-wrap msg-md-wrap--plaintext"
            }
        }>
            {/* 回答区 */}
            <div
                class=move || {
                    let loc = locale.get();
                    let apply = apply_assistant_display_filters.get();
                    let (is_loading, raw_len) = sessions.with(|list| {
                        let aid = active_id.get();
                        let m = list
                            .iter()
                            .find(|s| s.id == aid)
                            .and_then(|s| {
                                s.messages
                                    .iter()
                                    .find(|msg| msg.id == mid_stored.get_value())
                            });
                        match m {
                            Some(msg) => {
                                let len = message_text_for_display_ex(msg, loc, apply).chars().count();
                                (msg.state.as_deref() == Some("loading"), len)
                            }
                            None => (false, 0),
                        }
                    });
                    let long = !is_loading && raw_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD;
                    let mid = mid_stored.get_value();
                    let user_collapsed =
                        collapsed_long_assistant_ids.with(|v| v.iter().any(|id| id == &mid));
                    if long && user_collapsed {
                        "msg-md-split msg-md-answer msg-md-prose msg-md-prose-collapsed"
                    } else {
                        "msg-md-split msg-md-answer msg-md-prose"
                    }
                }
            >
                <div
                    class="msg-md-answer msg-body msg-md-prose"
                    node_ref=answer_body_ref
                ></div>
            </div>

            {/* 整条折叠按钮（作用于整个 msg-md-split，含思考区） */}
            <Show when=move || {
                let loc = locale.get();
                let apply = apply_assistant_display_filters.get();
                sessions.with(|list| {
                    let aid = active_id.get();
                    let Some(msg) = list
                        .iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| {
                            s.messages
                                .iter()
                                .find(|msg| msg.id == mid_stored.get_value())
                        })
                    else {
                        return false;
                    };
                    let is_loading = msg.state.as_deref() == Some("loading");
                    let raw_len = message_text_for_display_ex(msg, loc, apply).chars().count();
                    !is_loading && raw_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD
                })
            }>
                <button
                    type="button"
                    class="btn btn-muted btn-sm msg-md-toggle"
                    on:click=move |_| {
                        let b = mid_stored.get_value();
                        collapsed_long_assistant_ids.update(|v| {
                            if v.iter().any(|id| id == &b) {
                                v.retain(|id| id != &b);
                            } else {
                                v.push(b.clone());
                            }
                        });
                    }
                >
                    {move || {
                        let loc = locale.get();
                        let mid = mid_stored.get_value();
                        let user_collapsed =
                            collapsed_long_assistant_ids.with(|v| v.iter().any(|id| id == &mid));
                        if user_collapsed {
                            i18n::assistant_md_expand_full(loc)
                        } else {
                            i18n::assistant_md_collapse(loc)
                        }
                    }}
                </button>
            </Show>

        </div>
    }
}
