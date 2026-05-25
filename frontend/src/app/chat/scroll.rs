//! 主聊天区滚动：流式跟底、侧栏「在消息中打开」后滚入视图。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::messages_scroller_has_non_collapsed_selection;
use crate::session_search::scroll_message_into_view;
use crate::storage::ChatSession;

use crate::app::chat::message_virtual_viewport::sync_virtual_scroll_signals_from_element;
use crate::app::scroll_guard;

/// 流式增量计数：两次滚底之间若仍在持续增长，则跳过第二次 `scroll_height`（减少布局抖动）。
static MESSAGES_AUTO_SCROLL_GEN: AtomicU64 = AtomicU64::new(0);

/// 合并同一宏任务/短窗口内多次 Effect：仅保留一条在飞的跟底任务，避免每个 SSE chunk 各起一个 `spawn_local`。
static MESSAGES_SCROLL_TASK_PENDING: AtomicBool = AtomicBool::new(false);

/// 跟底指纹：只看活跃会话尾部若干条，避免流式时对整页消息 `fold` 全文长度（长会话下每个 chunk 同步开销大）。
fn active_session_tail_scroll_fingerprint(list: &[ChatSession], aid: &str) -> u64 {
    let Some(session) = list.iter().find(|s| s.id == aid) else {
        return 0;
    };
    let mut fp = session.messages.len() as u64;
    const TAIL: usize = 6;
    for msg in session.messages.iter().rev().take(TAIL) {
        fp = fp.wrapping_mul(41);
        fp = fp.wrapping_add(msg.id.len() as u64);
        fp = fp.wrapping_add(msg.text.len() as u64);
        fp = fp.wrapping_add(msg.reasoning_text.len() as u64);
        if let Some(st) = &msg.state {
            fp = fp.wrapping_add(st.to_wire().len() as u64);
        }
        fp = fp.wrapping_add(u64::from(msg.is_tool));
    }
    fp
}

fn scroll_messages_to_bottom_if_allowed(
    mref: &NodeRef<Div>,
    follow: &RwSignal<bool>,
    virtual_scroll_top: Option<RwSignal<i32>>,
    virtual_viewport_height: Option<RwSignal<i32>>,
) -> bool {
    if !follow.get_untracked() {
        return false;
    }
    let Some(el) = mref.get_untracked() else {
        return false;
    };
    if messages_scroller_has_non_collapsed_selection(&el) {
        return false;
    }
    el.set_scroll_top(el.scroll_height());
    if let (Some(top_sig), Some(vh_sig)) = (virtual_scroll_top, virtual_viewport_height) {
        sync_virtual_scroll_signals_from_element(&el, top_sig, vh_sig);
    }
    true
}

/// 测量消息列视口高度（供流式跟底时的尾部虚拟窗口；避免默认 600px 偏差）。
pub(crate) fn wire_messages_virtual_viewport_measure(
    messages_scroller: NodeRef<Div>,
    virtual_viewport_height: RwSignal<i32>,
) {
    Effect::new(move |_| {
        if let Some(el) = messages_scroller.get() {
            let h = el.client_height();
            if h > 0 {
                virtual_viewport_height.set(h);
            }
        }
    });
}

/// 消息列表指纹变化且开启自动跟底时，将滚动条置底（必要时二次对齐以覆盖换行后高度变化）。
pub(crate) fn wire_messages_auto_scroll(
    chat: ChatSessionSignals,
    messages_scroller: NodeRef<Div>,
    auto_scroll_chat: RwSignal<bool>,
    messages_scroll_from_effect: RwSignal<bool>,
    virtual_scroll_top: RwSignal<i32>,
    virtual_viewport_height: RwSignal<i32>,
) {
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let stream_text_overlay = chat.stream_text_overlay;
    Effect::new(move |_| {
        let aid = active_id.get();
        let _fingerprint = sessions.with(|list| active_session_tail_scroll_fingerprint(list, &aid));
        let _overlay = stream_text_overlay.get();

        if !auto_scroll_chat.get() {
            return;
        }

        MESSAGES_AUTO_SCROLL_GEN.fetch_add(1, Ordering::Relaxed);
        if MESSAGES_SCROLL_TASK_PENDING.swap(true, Ordering::AcqRel) {
            return;
        }

        let mref = messages_scroller;
        let follow = auto_scroll_chat;
        let scroll_from_effect = messages_scroll_from_effect;
        let vtop = virtual_scroll_top;
        let vvh = virtual_viewport_height;
        spawn_local(async move {
            let _scroll_from_effect_guard =
                scroll_guard::MessagesScrollFromEffectGuard::new(scroll_from_effect);

            let clear_task_pending =
                || MESSAGES_SCROLL_TASK_PENDING.store(false, Ordering::Release);

            TimeoutFuture::new(0).await;
            if !follow.get_untracked() {
                clear_task_pending();
                return;
            }

            let gen_after_yield = MESSAGES_AUTO_SCROLL_GEN.load(Ordering::Relaxed);
            if !scroll_messages_to_bottom_if_allowed(&mref, &follow, Some(vtop), Some(vvh)) {
                clear_task_pending();
                return;
            }

            // 流式仍高频更新时跳过第二次读 `scroll_height`，减轻主线程布局压力。
            TimeoutFuture::new(28).await;
            clear_task_pending();

            if MESSAGES_AUTO_SCROLL_GEN.load(Ordering::Relaxed) != gen_after_yield {
                return;
            }
            if !follow.get_untracked() {
                return;
            }
            let _ = scroll_messages_to_bottom_if_allowed(&mref, &follow, Some(vtop), Some(vvh));
        });
    });
}

/// 侧栏「在消息中打开」后滚动到对应气泡。
pub(crate) fn wire_focus_message_after_nav(focus_message_id_after_nav: RwSignal<Option<String>>) {
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
