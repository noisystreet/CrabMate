//! 助手回答区 DOM 绘制：`Effect`、增量流式追加与 Markdown 收尾（从 [`super::view`] 拆出以降低单函数 nloc）。
//!
//! - **流式 loading**：`insertAdjacentHTML('beforeend', …)` 仅追加新 token，不触碰已有 DOM
//! - **完成时**：`innerHTML` 一次性全量 Markdown 渲染

use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use super::helpers::AssistantMsgSnapshot;
use crate::message_render::fragment_to_chat_safe_html;

#[derive(Default)]
pub(super) struct SectionPaint {
    pub(super) latest_html: String,
    pub(super) raf_scheduled: bool,
    pub(super) stream_throttle_gen: u64,
    /// 已写入 DOM 的文本长度；流式时仅追加增量，避免全量 innerHTML 重建。
    pub(super) prev_text_len: usize,
    /// true: rAF 回调走 `set_inner_html` 全量替换；false: `insertAdjacentHTML` 增量追加。
    pub(super) is_replace: bool,
}

impl SectionPaint {
    pub(super) fn take_html(&mut self) -> String {
        std::mem::take(&mut self.latest_html)
    }
}

/// 增量追加：将 `html` 追加到元素末尾，不破坏已有 DOM 树。
fn answer_body_append_html(answer_body_ref: &NodeRef<Div>, html: &str) -> bool {
    let Some(node) = answer_body_ref.try_get_untracked().flatten() else {
        return false;
    };
    if let Some(he) = node.dyn_ref::<web_sys::HtmlElement>() {
        let _ = he.insert_adjacent_html("beforeend", html);
        return true;
    }
    false
}

/// 全量替换：`innerHTML`（用于流式完成后的 Markdown 终态渲染）。
fn answer_body_replace_html(answer_body_ref: &NodeRef<Div>, html: &str) -> bool {
    let Some(node) = answer_body_ref.try_get_untracked().flatten() else {
        return false;
    };
    if let Some(he) = node.dyn_ref::<web_sys::HtmlElement>() {
        he.set_inner_html(html);
        return true;
    }
    false
}

fn enqueue_answer_body_paint(
    paint_arc: &Arc<Mutex<SectionPaint>>,
    answer_body_ref: &NodeRef<Div>,
    html: String,
    is_replace: bool,
    stream_gen_gate: Option<u64>,
) {
    let paint_run = Arc::clone(paint_arc);
    let answer_body_ref = answer_body_ref.clone();
    {
        let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
        if let Some(expected) = stream_gen_gate {
            if g.stream_throttle_gen != expected {
                return;
            }
        }
        g.latest_html = html;
        g.is_replace = is_replace;
        if g.raf_scheduled {
            return;
        }
        g.raf_scheduled = true;
    }
    request_animation_frame(move || {
        let (html, is_replace) = {
            let mut g = paint_run.lock().expect("answer paint mutex poisoned");
            g.raf_scheduled = false;
            (g.take_html(), g.is_replace)
        };
        if html.is_empty() {
            return;
        }
        if is_replace {
            answer_body_replace_html(&answer_body_ref, &html);
        } else {
            answer_body_append_html(&answer_body_ref, &html);
        }
    });
}

fn snapshot_pair(snap: Option<AssistantMsgSnapshot>) -> (String, bool) {
    snap.map(|s| (s.display_text, s.is_loading))
        .unwrap_or_default()
}

/// [`super::view::assistant_markdown_collapsible_view`] 挂载回答区 `Effect` 所需的信号与节点。
pub(super) struct AssistantMarkdownAnswerEffectBundle {
    pub(super) snapshot_memo: Memo<Option<AssistantMsgSnapshot>>,
    pub(super) collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub(super) markdown_render: RwSignal<bool>,
    pub(super) answer_body_ref: NodeRef<Div>,
    pub(super) answer_paint: StoredValue<Arc<Mutex<SectionPaint>>>,
}

/// 供 [`super::view::assistant_markdown_collapsible_view`] 挂载：回答区 `Effect`。
pub(super) fn install_assistant_markdown_answer_effect(
    bundle: AssistantMarkdownAnswerEffectBundle,
) {
    let AssistantMarkdownAnswerEffectBundle {
        snapshot_memo,
        collapsed_long_assistant_ids,
        markdown_render,
        answer_body_ref,
        answer_paint,
    } = bundle;

    Effect::new({
        let answer_body_ref = answer_body_ref.clone();
        let answer_paint = answer_paint.clone();
        move |_| {
            let snap = snapshot_memo.get();
            let (text_src, is_loading) = snapshot_pair(snap);
            if is_loading {
                let _ = collapsed_long_assistant_ids.get_untracked();
            } else {
                let _ = collapsed_long_assistant_ids.get();
            }
            let _ = markdown_render.get();

            let throttle_gen = {
                let paint_arc = answer_paint.get_value();
                let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                g.stream_throttle_gen = g.stream_throttle_gen.wrapping_add(1);
                g.stream_throttle_gen
            };

            let paint_arc = answer_paint.get_value();

            if is_loading {
                // ── 流式：仅追加增量 ──
                let prev_len = {
                    let g = paint_arc.lock().expect("answer paint mutex poisoned");
                    g.prev_text_len
                };
                if text_src.len() > prev_len {
                    let new_text = &text_src[prev_len..];
                    let html = fragment_to_chat_safe_html(new_text, false);
                    {
                        let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                        g.prev_text_len = text_src.len();
                    }
                    enqueue_answer_body_paint(
                        &paint_arc,
                        &answer_body_ref,
                        html,
                        false, // append
                        Some(throttle_gen),
                    );
                }
                return;
            }

            // ── 完成：后台 Markdown 渲染，先让 UI 停止 typing dots ──
            {
                let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                g.prev_text_len = 0;
            }
            let md_on = markdown_render.get_untracked();
            let text = text_src.clone();
            let body_ref = answer_body_ref.clone();
            let arc = paint_arc.clone();
            spawn_local(async move {
                let html = fragment_to_chat_safe_html(&text, md_on);
                enqueue_answer_body_paint(&arc, &body_ref, html.clone(), true, None);
                answer_body_replace_html(&body_ref, &html);
            });
        }
    });
}
