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
    let thinking_body_ref = NodeRef::<Div>::new();
    let answer_body_ref = NodeRef::<Div>::new();
    let mid = message_id.clone();
    let mid_for_btn = message_id.clone();

    let thinking_paint = StoredValue::new(Arc::new(Mutex::new(SectionPaint::default())));
    let answer_paint = StoredValue::new(Arc::new(Mutex::new(SectionPaint::default())));

    // 思考区是否被用户收起（独立于整条折叠的子折叠）。
    let thinking_collapsed = StoredValue::new(false);

    // ---------- 思考区 Effect ----------
    Effect::new({
        let thinking_body_ref = thinking_body_ref.clone();
        let thinking_paint = thinking_paint.clone();
        let mid = mid.clone();
        move |_| {
            let _ = sessions.get();
            let _ = active_id.get();
            let _ = locale.get();
            let _ = markdown_render.get();
            let _ = apply_assistant_display_filters.get();
            let md_on = markdown_render.get_untracked();

            let reasoning_src = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| m.reasoning_text.clone())
                    .unwrap_or_default()
            });

            let thinking_plain = reasoning_src.trim();
            if thinking_plain.is_empty() {
                return;
            }

            let html = fragment_to_chat_safe_html(thinking_plain, md_on);
            let paint_arc = thinking_paint.get_value();
            {
                let mut g = paint_arc.lock().expect("thinking paint mutex poisoned");
                if g.raf_scheduled {
                    return;
                }
                g.latest_html = html;
                g.raf_scheduled = true;
            }
            let paint_run = Arc::clone(&paint_arc);
            let thinking_body_ref = thinking_body_ref.clone();
            request_animation_frame(move || {
                let html = {
                    let mut g = paint_run.lock().expect("thinking paint mutex poisoned");
                    g.raf_scheduled = false;
                    g.take_html()
                };
                if let Some(n) = thinking_body_ref.get_untracked()
                    && let Some(he) = n.dyn_ref::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });

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
            let md_on = markdown_render.get_untracked();

            let text_src = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| m.text.clone())
                    .unwrap_or_default()
            });

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
            {/* 思考区：仅当有思维链内容时才渲染 */}
            <Show when=move || {
                sessions.with(|list| {
                    let aid = active_id.get();
                    list.iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| {
                            s.messages.iter().find(|msg| msg.id == mid_stored.get_value())
                        })
                        .map(|m| !m.reasoning_text.trim().is_empty())
                        .unwrap_or(false)
                })
            }>
                <div
                    class=move || {
                        let is_loading = sessions.with(|list| {
                            let aid = active_id.get();
                            list.iter()
                                .find(|s| s.id == aid)
                                .and_then(|s| {
                                    s.messages
                                        .iter()
                                        .find(|msg| msg.id == mid_stored.get_value())
                                })
                                .map(|m| m.state.as_deref() == Some("loading"))
                                .unwrap_or(false)
                        });
                        let collapsed = thinking_collapsed.get_value();
                        let base = if collapsed { "msg-md-thinking msg-md-thinking--collapsed" } else { "msg-md-thinking" };
                        if is_loading {
                            format!("{base} streaming")
                        } else {
                            base.to_string()
                        }
                    }
                >
                    {/* 思考区头部：标签 + 展开/收起 */}
                    <div class="msg-md-thinking-header">
                        <span class="msg-md-thinking-label">
                            {move || i18n::assistant_thinking_section_label(locale.get()) }
                        </span>
                        <button
                            type="button"
                            class="btn btn-muted btn-sm msg-md-thinking-toggle"
                            on:click=move |_| {
                                let cur = thinking_collapsed.get_value();
                                thinking_collapsed.set_value(!cur);
                            }
                        >
                            {move || {
                                if thinking_collapsed.get_value() {
                                    i18n::assistant_thinking_expand(locale.get())
                                } else {
                                    i18n::assistant_thinking_collapse(locale.get())
                                }
                            }}
                        </button>
                    </div>
                    {/* 思考内容 */}
                    <div class="msg-md-thinking-body" node_ref=thinking_body_ref></div>
                </div>
            </Show>

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

            {/* 流式打字指示器（loading 状态） */}
            <Show when=move || {
                sessions.with(|list| {
                    let aid = active_id.get();
                    list.iter()
                        .find(|s| s.id == aid)
                        .and_then(|s| {
                            s.messages
                                .iter()
                                .find(|msg| msg.id == mid_stored.get_value())
                        })
                        .map(|m| m.state.as_deref() == Some("loading"))
                        .unwrap_or(false)
                })
            }>
                <span class="typing-dots" aria-hidden="true">
                    <span></span>
                    <span></span>
                    <span></span>
                </span>
            </Show>
        </div>
    }
}
