//! [`assistant_markdown_collapsible_view`]：助手气泡 DOM 写入与折叠 UI。

use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::message_render::fragment_to_chat_safe_html;
use crate::storage::ChatSession;

use super::helpers::{LONG_ASSISTANT_COLLAPSE_THRESHOLD, snapshot_assistant_message_for_mid};

/// 流式生成阶段两次写入助手回答区 DOM 的最小间隔（毫秒），降低 SSE 高频增量时的纯文本转义与布局开销。
const STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS: u32 = 72;

#[inline]
fn js_performance_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[derive(Default)]
struct SectionPaint {
    latest_html: String,
    raf_scheduled: bool,
    /// 每次 Effect 或尾随定时刷新开始时递增； trailing timer 仅在仍等于派发时的值时才绘 DOM，避免过期回调盖住终态 Markdown。
    stream_throttle_gen: u64,
    /// 上一次流式阶段实际写入回答区的时间戳（`performance.now()`）；`0` 表示尚未写过。
    last_stream_paint_ms: f64,
}

impl SectionPaint {
    fn take_html(&mut self) -> String {
        std::mem::take(&mut self.latest_html)
    }
}

fn enqueue_answer_body_paint(
    paint_arc: &Arc<Mutex<SectionPaint>>,
    answer_body_ref: &NodeRef<Div>,
    html: String,
) {
    let paint_run = Arc::clone(paint_arc);
    let answer_body_ref = answer_body_ref.clone();
    {
        let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
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
        // rAF 可能在消息行已卸载后执行；此时 NodeRef 已 dispose，`get_untracked` 会 panic。
        let Some(node) = answer_body_ref.try_get_untracked().flatten() else {
            return;
        };
        if let Some(he) = node.dyn_ref::<web_sys::HtmlElement>() {
            he.set_inner_html(&html);
        }
    });
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

    // 回答区 Effect：`Effect` **顶层**对下列信号做 `.get()` 以建立订阅；块内对 `active_id` /
    // `locale` 等再用 `get_untracked`，避免在 `sessions.with` 的子追踪域里重复注册同一依赖，
    // 并与本轮 `sessions` 快照一致（见模块 `mod.rs` 说明）。
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

            let throttle_gen = {
                let paint_arc = answer_paint.get_value();
                let mut g = paint_arc.lock().expect("answer paint mutex poisoned");
                g.stream_throttle_gen = g.stream_throttle_gen.wrapping_add(1);
                g.stream_throttle_gen
            };

            let (text_src, is_loading) = sessions.with(|list| {
                let aid = active_id.get_untracked();
                let loc = locale.get_untracked();
                let apply = apply_assistant_display_filters.get_untracked();
                snapshot_assistant_message_for_mid(list, &aid, &mid, loc, apply)
                    .map(|s| (s.display_text, s.is_loading))
                    .unwrap_or_default()
            });

            // 流式生成中先按纯文本渲染，避免半截 Markdown（尤其未闭合代码围栏）
            // 在不同浏览器里触发布局伪影（如黑条/闪动）；完成后自动切回 Markdown。
            let md_on = markdown_render.get_untracked() && !is_loading;
            let paint_arc = answer_paint.get_value();

            if !is_loading {
                let html = fragment_to_chat_safe_html(&text_src, md_on);
                enqueue_answer_body_paint(&paint_arc, &answer_body_ref, html);
                return;
            }

            // 流式阶段：时间节流 + rAF 合并，避免每个 SSE 片段都跑一遍转义并触发布局。
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
                enqueue_answer_body_paint(&paint_arc, &answer_body_ref, html);
                return;
            }

            let wait_ms = ((f64::from(STREAM_ASSISTANT_DOM_MIN_INTERVAL_MS) - elapsed_ms).ceil()
                as u32)
                .max(1);

            let paint_arc_timer = Arc::clone(&paint_arc);
            let answer_body_ref_timer = answer_body_ref.clone();
            let sessions_t = sessions;
            let active_id_t = active_id;
            let locale_t = locale;
            let apply_t = apply_assistant_display_filters;
            let markdown_render_t = markdown_render;
            let mid_t = mid.clone();

            spawn_local(async move {
                TimeoutFuture::new(wait_ms).await;
                let snap = sessions_t.with(|list| {
                    let aid = active_id_t.get_untracked();
                    let loc = locale_t.get_untracked();
                    let apply = apply_t.get_untracked();
                    snapshot_assistant_message_for_mid(list, &aid, &mid_t, loc, apply)
                });
                let Some(s) = snap else {
                    return;
                };
                if !s.is_loading {
                    return;
                }
                {
                    let g = paint_arc_timer.lock().expect("answer paint mutex poisoned");
                    if g.stream_throttle_gen != throttle_gen {
                        return;
                    }
                }
                let md_on_t = markdown_render_t.get_untracked() && !s.is_loading;
                let html = fragment_to_chat_safe_html(&s.display_text, md_on_t);
                let now_after = js_performance_now_ms();
                {
                    let mut g = paint_arc_timer.lock().expect("answer paint mutex poisoned");
                    if g.stream_throttle_gen != throttle_gen {
                        return;
                    }
                    g.last_stream_paint_ms = now_after;
                }
                enqueue_answer_body_paint(&paint_arc_timer, &answer_body_ref_timer, html);
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
                        snapshot_assistant_message_for_mid(
                            list,
                            &aid,
                            mid_stored.get_value().as_str(),
                            loc,
                            apply,
                        )
                        .map(|s| (s.is_loading, s.display_char_len))
                        .unwrap_or((false, 0))
                    });
                    let long =
                        !is_loading && raw_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD;
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
                    snapshot_assistant_message_for_mid(
                        list,
                        &aid,
                        mid_stored.get_value().as_str(),
                        loc,
                        apply,
                    )
                    .is_some_and(|s| {
                        !s.is_loading && s.display_char_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD
                    })
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
