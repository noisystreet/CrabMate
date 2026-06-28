//! 助手回答区 DOM 绘制：`Effect`、流式节流与 rAF 合并（从 [`super::view`] 拆出以降低单函数 nloc）。

use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use super::helpers::AssistantMsgSnapshot;
use crate::message_render::fragment_to_chat_safe_html;

/// 流式生成阶段两次写入助手回答区 DOM 的最小间隔（毫秒）。
pub(super) const STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS: u32 = 72;

#[inline]
fn js_performance_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[derive(Default)]
pub(super) struct SectionPaint {
    pub(super) latest_html: String,
    pub(super) raf_scheduled: bool,
    pub(super) stream_throttle_gen: u64,
    pub(super) last_stream_paint_ms: f64,
}

impl SectionPaint {
    pub(super) fn take_html(&mut self) -> String {
        std::mem::take(&mut self.latest_html)
    }
}

fn flush_answer_body_html_now(answer_body_ref: &NodeRef<Div>, html: &str) -> bool {
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
            g.last_stream_paint_ms = js_performance_now_ms();
        }
        g.latest_html = html;
        if g.raf_scheduled {
            return;
        }
        g.raf_scheduled = true;
    }
    request_animation_frame(move || {
        let html = {
            let mut g = paint_run.lock().expect("answer paint mutex poisoned");
            g.raf_scheduled = false;
            g.take_html()
        };
        if html.is_empty() {
            return;
        }
        let Some(node) = answer_body_ref.try_get_untracked().flatten() else {
            return;
        };
        if let Some(he) = node.dyn_ref::<web_sys::HtmlElement>() {
            he.set_inner_html(&html);
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

/// 供 [`super::view::assistant_markdown_collapsible_view`] 挂载：回答区 `Effect` 与流式节流。
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
            let is_loading = snap.as_ref().is_some_and(|s| s.is_loading);
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

            let (text_src, is_loading) = snapshot_pair(snap);

            let md_on = markdown_render.get_untracked() && !is_loading;
            let paint_arc = answer_paint.get_value();

            if !is_loading {
                let html = fragment_to_chat_safe_html(&text_src, md_on);
                enqueue_answer_body_paint(&paint_arc, &answer_body_ref, html.clone(), None);
                if flush_answer_body_html_now(&answer_body_ref, html.as_str()) {
                    let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                    let _ = g.take_html();
                }
                return;
            }

            let now = js_performance_now_ms();
            let elapsed_ms = {
                let g = paint_arc.lock().expect("answer paint mutex poisoned");
                if g.last_stream_paint_ms == 0.0 {
                    f64::from(STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS)
                } else {
                    (now - g.last_stream_paint_ms).max(0.0)
                }
            };

            if elapsed_ms >= f64::from(STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS) {
                {
                    let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                    g.last_stream_paint_ms = now;
                }
                let html = fragment_to_chat_safe_html(&text_src, md_on);
                enqueue_answer_body_paint(&paint_arc, &answer_body_ref, html, None);
                return;
            }

            let wait_ms = ((f64::from(STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS) - elapsed_ms).ceil()
                as u32)
                .max(1);

            let paint_arc_timer = Arc::clone(&paint_arc);
            let answer_body_ref_timer = answer_body_ref.clone();
            let snapshot_memo_t = snapshot_memo;
            let markdown_render_t = markdown_render;

            spawn_local(async move {
                TimeoutFuture::new(wait_ms).await;
                {
                    let g = paint_arc_timer.lock().expect("answer paint mutex poisoned");
                    if g.stream_throttle_gen != throttle_gen {
                        return;
                    }
                }
                let snap = snapshot_memo_t.get_untracked();
                let Some(s) = snap else {
                    return;
                };
                if !s.is_loading {
                    return;
                }
                let md_on_t = markdown_render_t.get_untracked() && !s.is_loading;
                let html = fragment_to_chat_safe_html(&s.display_text, md_on_t);
                enqueue_answer_body_paint(
                    &paint_arc_timer,
                    &answer_body_ref_timer,
                    html,
                    Some(throttle_gen),
                );
            });
        }
    });
}
