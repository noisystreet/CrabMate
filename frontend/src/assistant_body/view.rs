//! [`assistant_markdown_collapsible_view`]：助手气泡 DOM 写入与折叠 UI。

use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::request_animation_frame;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Locale};
use crate::message_render::fragment_to_chat_safe_html;
use crate::storage::ChatSession;

use super::helpers::{LONG_ASSISTANT_COLLAPSE_THRESHOLD, snapshot_assistant_message_for_mid};

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
                // rAF 可能在消息行已卸载后执行；此时 NodeRef 已 dispose，`get_untracked` 会 panic。
                let Some(node) = answer_body_ref.try_get_untracked().flatten() else {
                    return;
                };
                if let Some(he) = node.dyn_ref::<web_sys::HtmlElement>() {
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
