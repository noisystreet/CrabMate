//! 主聊天区滚动：流式跟底、侧栏「在消息中打开」后滚入视图。

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::session_search::scroll_message_into_view;
use crate::storage::ChatSession;

use super::scroll_guard;

/// 合并同一短窗口内多次触发的跟底：仅保留「最新一次」effect 入队的滚动任务，避免尾包文本 + 收尾 state 连续更新导致多次 `set_scroll_top` 抖动。
static MESSAGES_AUTO_SCROLL_GEN: AtomicU64 = AtomicU64::new(0);

/// 消息列表指纹变化且开启自动跟底时，将滚动条置底（多帧以覆盖流式换行后高度变化）。
pub(super) fn wire_messages_auto_scroll(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    messages_scroller: NodeRef<Div>,
    auto_scroll_chat: RwSignal<bool>,
    messages_scroll_from_effect: RwSignal<bool>,
) {
    Effect::new(move |_| {
        let aid = active_id.get();
        let _fingerprint = sessions.with(|list| {
            list.iter()
                .find(|s| s.id == aid)
                .map(|s| {
                    s.messages
                        .iter()
                        .fold(0u64, |acc, m| acc.wrapping_add(m.text.len() as u64))
                        .wrapping_add((s.messages.len() as u64).saturating_mul(17))
                })
                .unwrap_or(0)
        });

        if !auto_scroll_chat.get() {
            return;
        }

        let generation = MESSAGES_AUTO_SCROLL_GEN
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let mref = messages_scroller;
        let follow = auto_scroll_chat;
        let scroll_from_effect = messages_scroll_from_effect;
        spawn_local(async move {
            let _scroll_from_effect_guard =
                scroll_guard::MessagesScrollFromEffectGuard::new(scroll_from_effect);
            if !follow.get_untracked() {
                return;
            }
            TimeoutFuture::new(0).await;
            if MESSAGES_AUTO_SCROLL_GEN.load(Ordering::Relaxed) != generation {
                return;
            }
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                if !messages_scroller_has_non_collapsed_selection(&el) {
                    el.set_scroll_top(el.scroll_height());
                }
            }
            TimeoutFuture::new(0).await;
            if MESSAGES_AUTO_SCROLL_GEN.load(Ordering::Relaxed) != generation {
                return;
            }
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                if !messages_scroller_has_non_collapsed_selection(&el) {
                    el.set_scroll_top(el.scroll_height());
                }
            }
            // 再等一帧：流式换行后布局高度可能在本轮 paint 后才稳定
            TimeoutFuture::new(16).await;
            if MESSAGES_AUTO_SCROLL_GEN.load(Ordering::Relaxed) != generation {
                return;
            }
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                if !messages_scroller_has_non_collapsed_selection(&el) {
                    el.set_scroll_top(el.scroll_height());
                }
            }
        });
    });
}

/// 侧栏「在消息中打开」后滚动到对应气泡。
pub(super) fn wire_focus_message_after_nav(focus_message_id_after_nav: RwSignal<Option<String>>) {
    Effect::new({
        let focus_message_id_after_nav = focus_message_id_after_nav;
        move |_| {
            let Some(mid) = focus_message_id_after_nav.get() else {
                return;
            };
            focus_message_id_after_nav.set(None);
            let mid = mid.clone();
            spawn_local(async move {
                TimeoutFuture::new(48).await;
                scroll_message_into_view(&mid);
                TimeoutFuture::new(120).await;
                scroll_message_into_view(&mid);
            });
        }
    });
}
