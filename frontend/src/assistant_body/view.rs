//! [`assistant_markdown_collapsible_view`]：助手气泡 DOM 写入与折叠 UI。

use leptos::html::Div;
use leptos::prelude::*;

use std::sync::{Arc, Mutex};

use crate::i18n::{self, Locale};
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;

use super::helpers::{LONG_ASSISTANT_COLLAPSE_THRESHOLD, assistant_markdown_display_memo};
use super::md_answer_effect::{
    AssistantMarkdownAnswerEffectBundle, SectionPaint, install_assistant_markdown_answer_effect,
};

/// 供 [`assistant_markdown_collapsible_view`] 入参聚合（避免形参个数棘轮与 `clippy::too_many_arguments`）。
#[derive(Clone)]
pub struct AssistantMarkdownCollapsibleWire {
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub message_id: String,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
    pub stream_overlay_display_mid: RwSignal<Option<String>>,
}

/// 助手非工具消息：Markdown → 净化 HTML；思维链独立区域 + 终答区。
pub fn assistant_markdown_collapsible_view(w: AssistantMarkdownCollapsibleWire) -> impl IntoView {
    let AssistantMarkdownCollapsibleWire {
        sessions,
        active_id,
        message_id,
        collapsed_long_assistant_ids,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        stream_text_overlay,
        stream_overlay_display_mid,
    } = w;
    let answer_body_ref = NodeRef::<Div>::new();
    let mid_for_btn = message_id.clone();

    let answer_paint = StoredValue::new(Arc::new(Mutex::new(SectionPaint::default())));

    let snapshot_memo = assistant_markdown_display_memo(
        sessions,
        active_id,
        message_id.clone(),
        stream_text_overlay,
        stream_overlay_display_mid,
        locale,
        apply_assistant_display_filters,
    );

    install_assistant_markdown_answer_effect(AssistantMarkdownAnswerEffectBundle {
        snapshot_memo,
        collapsed_long_assistant_ids,
        markdown_render,
        answer_body_ref: answer_body_ref.clone(),
        answer_paint: answer_paint.clone(),
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
                    let (is_loading, raw_len) = snapshot_memo
                        .get()
                        .map(|s| (s.is_loading, s.display_char_len))
                        .unwrap_or((false, 0));
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
                snapshot_memo.get().is_some_and(|s| {
                    !s.is_loading && s.display_char_len >= LONG_ASSISTANT_COLLAPSE_THRESHOLD
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
