//! IDE 内确认对话框（替代 `window.confirm`，桌面 WebView 与 E2E 更可靠）。

use std::cell::Cell;

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

thread_local! {
    static NEXT_CONFIRM_ID: Cell<u64> = const { Cell::new(1) };
}

fn next_confirm_id() -> u64 {
    NEXT_CONFIRM_ID.with(|c| {
        let id = c.get();
        c.set(id.saturating_add(1));
        id
    })
}

/// 待展示的确认请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeConfirmPrompt {
    pub id: u64,
    pub message: String,
}

/// 用户对某次请求的应答。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdeConfirmResult {
    pub id: u64,
    pub ok: bool,
}

/// 确认框信号（挂于 [`crate::app::app_signals::IdeChromeSignals`]）。
#[derive(Clone, Copy)]
pub struct IdeConfirmSignals {
    pub pending: RwSignal<Option<IdeConfirmPrompt>>,
    pub result: RwSignal<Option<IdeConfirmResult>>,
}

/// 展示确认框并异步等待用户选择；取消或关闭返回 `false`。
pub async fn ide_confirm_user(signals: IdeConfirmSignals, message: String) -> bool {
    let id = next_confirm_id();
    signals.pending.set(Some(IdeConfirmPrompt {
        id,
        message: message.clone(),
    }));
    loop {
        TimeoutFuture::new(16).await;
        if let Some(r) = signals.result.get_untracked() {
            if r.id == id {
                signals.result.set(None);
                signals.pending.set(None);
                return r.ok;
            }
        }
        if signals.pending.get_untracked().is_none() {
            return false;
        }
    }
}

/// 由确认框 UI 调用：写入结果并关闭。
pub fn resolve_ide_confirm(signals: IdeConfirmSignals, ok: bool) {
    let Some(p) = signals.pending.get_untracked() else {
        return;
    };
    signals.result.set(Some(IdeConfirmResult { id: p.id, ok }));
    signals.pending.set(None);
}

/// 无待处理请求时由 Escape 等调用。
pub fn dismiss_ide_confirm(signals: IdeConfirmSignals) {
    if let Some(p) = signals.pending.get_untracked() {
        signals.result.set(Some(IdeConfirmResult {
            id: p.id,
            ok: false,
        }));
        signals.pending.set(None);
    }
}
