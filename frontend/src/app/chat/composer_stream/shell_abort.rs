//! [`super::handles::ComposerStreamShell`] 上 `AbortController` 与用户取消标志的 Mutex 集中读写，
//! 避免 `attach` / 回调 / 停止按钮各处重复 `lock().unwrap()`。
//! 槽位定义见 **[`crate::app::app_signals::StreamControlSignals`]**（**`abort_cell`** / **`user_cancelled_stream`**）；此处仅封装业务语义。
//! `spawn_local` 尾逻辑请用 [`user_cancelled_flag`]，勿在闭包外再 `Arc::clone`  Mutex 手动锁。

use leptos::prelude::Update;

use super::super::handles::ComposerStreamShell;

fn bump_stream_abort_epoch(shell: &ComposerStreamShell) {
    shell.stream.stream_abort_epoch.update(|n| {
        *n = n.wrapping_add(1);
    });
}

/// 发起新流前：中止上一控制器、清除「用户取消」标记（随后应 [`store_abort_controller`]）。
pub(super) fn reset_abort_state_for_new_attach(shell: &ComposerStreamShell) {
    let prev = shell.stream.abort_cell.lock().unwrap().take();
    if prev.is_some() {
        bump_stream_abort_epoch(shell);
    }
    if let Some(ac) = prev {
        ac.abort();
    }
    *shell.stream.user_cancelled_stream.lock().unwrap() = false;
}

pub(super) fn store_abort_controller(shell: &ComposerStreamShell, ac: web_sys::AbortController) {
    *shell.stream.abort_cell.lock().unwrap() = Some(ac);
    bump_stream_abort_epoch(shell);
}

pub(crate) fn clear_abort_slot(shell: &ComposerStreamShell) {
    let taken = shell.stream.abort_cell.lock().unwrap().take();
    if taken.is_some() {
        bump_stream_abort_epoch(shell);
    }
}

pub(super) fn user_cancelled_flag(shell: &ComposerStreamShell) -> bool {
    *shell.stream.user_cancelled_stream.lock().unwrap()
}

/// 用户点击停止：若当前无在途流则返回 `false`；否则置取消标志、取出并 `abort` 控制器。
pub(crate) fn user_cancel_in_flight_stream(shell: &ComposerStreamShell) -> bool {
    if shell.stream.abort_cell.lock().unwrap().is_none() {
        return false;
    }
    *shell.stream.user_cancelled_stream.lock().unwrap() = true;
    if let Some(ac) = shell.stream.abort_cell.lock().unwrap().take() {
        ac.abort();
        bump_stream_abort_epoch(shell);
    }
    true
}
