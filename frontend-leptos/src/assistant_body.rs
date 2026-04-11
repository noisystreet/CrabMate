//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复可折叠。

use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::markdown;
use crate::message_format::{
    assistant_text_for_display, assistant_thinking_body_and_answer_raw,
    filter_assistant_thinking_markers_for_display, message_text_for_display_ex,
};
use crate::storage::ChatSession;

/// 超过该字符数（按展示用 `message_text_for_display_ex` 与当前 `apply_assistant_display_filters` 计）的已完成助手消息默认折叠。
const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

#[derive(Default)]
struct AssistantMdPaint {
    latest_html: String,
    raf_scheduled: bool,
    /// 本帧内是否曾出现「由空到有字」的流式首包（用于一次性淡入 class）。
    pending_first_chunk_anim: bool,
}

/// 与 `message_text_for_display_ex` 一致：思维链与终答拼成一段再渲染（无单独「思考过程」容器）。
fn combined_assistant_display_plain(thinking_trimmed: &str, answer_display: &str) -> String {
    if thinking_trimmed.is_empty() {
        return answer_display.to_string();
    }
    if answer_display.trim().is_empty() {
        return thinking_trimmed.to_string();
    }
    format!("{thinking_trimmed}\n\n{answer_display}")
}

/// 助手非工具消息：Markdown → 净化 HTML；可选折叠长文。
pub fn assistant_markdown_collapsible_view(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let split_ref = NodeRef::<Div>::new();
    let body_ref = NodeRef::<Div>::new();
    let mid = message_id.clone();
    let mid_for_btn = message_id.clone();
    let prev_raw = StoredValue::new(Arc::new(Mutex::new(String::new())));
    let paint = StoredValue::new(Arc::new(Mutex::new(AssistantMdPaint::default())));

    Effect::new({
        let split_ref = split_ref.clone();
        let body_ref = body_ref.clone();
        let mid = mid.clone();
        move |_| {
            let _ = sessions.get();
            let _ = active_id.get();
            let _ = expanded_long_assistant_ids.get();
            let _ = locale.get();
            let _ = markdown_render.get();
            let _ = apply_assistant_display_filters.get();
            let loc = locale.get_untracked();
            let md_on = markdown_render.get_untracked();
            let apply = apply_assistant_display_filters.get_untracked();
            let (reasoning_src, text_src, is_loading) = sessions.with(|list| {
                let aid = active_id.get_untracked();
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|msg| msg.id == mid))
                    .map(|m| {
                        (
                            m.reasoning_text.clone(),
                            m.text.clone(),
                            m.state.as_deref() == Some("loading"),
                        )
                    })
                    .unwrap_or_default()
            });
            let filtered_reasoning_and_text = if apply {
                Some((
                    filter_assistant_thinking_markers_for_display(&reasoning_src, is_loading),
                    filter_assistant_thinking_markers_for_display(&text_src, is_loading),
                ))
            } else {
                None
            };
            let (thinking_raw, answer_raw) = match &filtered_reasoning_and_text {
                Some((rs, tx)) => {
                    assistant_thinking_body_and_answer_raw(rs.as_str(), tx.as_str(), true)
                }
                None => assistant_thinking_body_and_answer_raw(&reasoning_src, &text_src, false),
            };
            let r_trim = thinking_raw.trim();
            let answer_display = assistant_text_for_display(answer_raw, is_loading, loc, apply);
            let combined = combined_assistant_display_plain(r_trim, answer_display.as_str());
            let snapshot = combined.clone();
            let first_stream_chunk = {
                let arc = prev_raw.get_value();
                let mut g = arc.lock().expect("assistant prev_raw mutex poisoned");
                let prev_empty = g.is_empty();
                let now_nonempty = !snapshot.trim().is_empty();
                let first = prev_empty && now_nonempty && is_loading;
                *g = snapshot;
                first
            };
            let html = if combined.trim().is_empty() {
                String::new()
            } else if md_on {
                markdown::to_safe_html(&combined)
            } else {
                markdown::plaintext_to_safe_html(&combined)
            };
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
            let split_ref = split_ref.clone();
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
                if let Some(n) = split_ref.get()
                    && let Some(he) = n.dyn_ref::<web_sys::HtmlElement>()
                {
                    let _ = he.class_list().remove_1("msg-md-first-chunk");
                    if do_first {
                        let _ = he.class_list().add_1("msg-md-first-chunk");
                    }
                }
                if let Some(n) = body_ref.get()
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
                            Some(msg) => (
                                msg.state.as_deref() == Some("loading"),
                                message_text_for_display_ex(msg, loc, apply).chars().count(),
                            ),
                            None => (false, 0),
                        }
                    });
                    let long = !is_loading && raw_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD;
                    let mid = mid_stored.get_value();
                    let expanded =
                        expanded_long_assistant_ids.with(|v| v.iter().any(|id| id == &mid));
                    let collapsed = long && !expanded;
                    if collapsed {
                        "msg-md-split msg-md-prose-collapsed"
                    } else {
                        "msg-md-split"
                    }
                }
                node_ref=split_ref
            >
                <div
                    class="msg-md-answer msg-body msg-md-prose"
                    node_ref=body_ref
                ></div>
            </div>
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
                        expanded_long_assistant_ids.update(|v| {
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
                        let expanded =
                            expanded_long_assistant_ids.with(|v| v.iter().any(|id| id == &mid));
                        if expanded {
                            i18n::assistant_md_collapse(loc)
                        } else {
                            i18n::assistant_md_expand_full(loc)
                        }
                    }}
                </button>
            </Show>
        </div>
    }
}
