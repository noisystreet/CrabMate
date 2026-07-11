//! 助手回答区 DOM 绘制：`Effect`、增量流式追加与 Markdown 收尾（从 [`super::view`] 拆出以降低单函数 nloc）。
//!
//! - **流式 loading**：`insertAdjacentHTML('beforeend', …)` 仅追加新 token，不触碰已有 DOM
//! - **流式节流**：两次 DOM 写入至少相隔 [`STREAM_DOM_MIN_INTERVAL_MS`]（与 [`super::mod`] 文档一致）
//! - **完成时**：`innerHTML` 一次性全量 Markdown 渲染

use std::sync::{Arc, Mutex};

use gloo_timers::callback::Timeout;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use super::helpers::AssistantMsgSnapshot;
use crate::debounce_schedule::debounce_should_apply;
use crate::message_render::fragment_to_chat_safe_html;

/// 流式回答区两次 DOM 写入的最小间隔（毫秒）。
pub(super) const STREAM_DOM_MIN_INTERVAL_MS: u32 = 72;

#[derive(Default)]
pub(super) struct SectionPaint {
    pub(super) latest_html: String,
    pub(super) raf_scheduled: bool,
    pub(super) stream_throttle_gen: u64,
    /// 已写入 DOM 的文本长度；流式时仅追加增量，避免全量 innerHTML 重建。
    pub(super) prev_text_len: usize,
    /// true: rAF 回调走 `set_inner_html` 全量替换；false: `insertAdjacentHTML` 增量追加。
    pub(super) is_replace: bool,
    /// 节流窗口内合并的 HTML 片段（尚未写入 DOM）。
    pub(super) pending_stream_html: String,
    /// 与 `pending_stream_html` 对应的 `display_text` 已消费长度。
    pub(super) pending_stream_through_len: usize,
    /// 流式 DOM 防抖代数（见 [`crate::debounce_schedule`]）。
    pub(super) stream_flush_generation: u64,
    pub(super) last_stream_dom_write_ms: f64,
}

impl SectionPaint {
    pub(super) fn take_html(&mut self) -> String {
        std::mem::take(&mut self.latest_html)
    }

    pub(super) fn cancel_stream_flush(&mut self) {
        self.stream_flush_generation = self.stream_flush_generation.wrapping_add(1);
        self.pending_stream_html.clear();
        self.pending_stream_through_len = 0;
        self.stream_throttle_gen = self.stream_throttle_gen.wrapping_add(1);
    }
}

fn performance_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
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

fn flush_pending_stream_dom(
    paint_arc: &Arc<Mutex<SectionPaint>>,
    answer_body_ref: &NodeRef<Div>,
    when_scheduled: u64,
) {
    let html = {
        let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
        if !debounce_should_apply(when_scheduled, g.stream_flush_generation) {
            return;
        }
        if g.pending_stream_html.is_empty() {
            return;
        }
        let html = std::mem::take(&mut g.pending_stream_html);
        g.prev_text_len = g.pending_stream_through_len;
        g.last_stream_dom_write_ms = performance_now_ms();
        html
    };
    enqueue_answer_body_paint(paint_arc, answer_body_ref, html, false, None);
}

fn schedule_pending_stream_dom_flush(
    paint_arc: &Arc<Mutex<SectionPaint>>,
    answer_body_ref: &NodeRef<Div>,
    when_scheduled: u64,
) {
    let paint_run = Arc::clone(paint_arc);
    let answer_body_ref = answer_body_ref.clone();
    Timeout::new(STREAM_DOM_MIN_INTERVAL_MS, move || {
        flush_pending_stream_dom(&paint_run, &answer_body_ref, when_scheduled);
    })
    .forget();
}

fn queue_stream_text_append(
    paint_arc: &Arc<Mutex<SectionPaint>>,
    answer_body_ref: &NodeRef<Div>,
    html: String,
    through_len: usize,
) {
    let when_scheduled = {
        let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
        g.pending_stream_html.push_str(&html);
        g.pending_stream_through_len = through_len;
        g.stream_flush_generation = g.stream_flush_generation.wrapping_add(1);
        g.stream_flush_generation
    };
    schedule_pending_stream_dom_flush(paint_arc, answer_body_ref, when_scheduled);
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

            let paint_arc = answer_paint.get_value();

            if is_loading {
                // ── 流式：合并增量，按 STREAM_DOM_MIN_INTERVAL_MS 尾随写入 DOM ──
                let prev_len = {
                    let g = paint_arc.lock().expect("answer paint mutex poisoned");
                    g.prev_text_len
                };
                if text_src.len() > prev_len {
                    let new_text = &text_src[prev_len..];
                    let html = fragment_to_chat_safe_html(new_text, false);
                    queue_stream_text_append(&paint_arc, &answer_body_ref, html, text_src.len());
                }
                return;
            }

            // ── 完成：取消流式尾随刷新，后台 Markdown 终态 ──
            {
                let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                g.cancel_stream_flush();
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

#[cfg(test)]
mod tests {
    use super::{STREAM_DOM_MIN_INTERVAL_MS, SectionPaint};

    #[test]
    fn stream_dom_min_interval_matches_module_doc() {
        assert_eq!(STREAM_DOM_MIN_INTERVAL_MS, 72);
    }

    #[test]
    fn cancel_stream_flush_bumps_generation() {
        let mut paint = SectionPaint {
            stream_flush_generation: 5,
            stream_throttle_gen: 2,
            pending_stream_html: "x".to_string(),
            ..SectionPaint::default()
        };
        paint.cancel_stream_flush();
        assert_eq!(paint.stream_flush_generation, 6);
        assert_eq!(paint.stream_throttle_gen, 3);
        assert!(paint.pending_stream_html.is_empty());
    }
}
