//! App 壳层接线的**单一入口**：[`bootstrap_app_shell`] 构造 [`AppSignals`] 后调用
//! [`run_shell_wiring_in_order`](super::app_shell_wire_phases::run_shell_wiring_in_order)，
//! 再组装 [`AppShellCtx`]。阶段语义见 [`super::app_shell_wire_phases`]；聊天主列内部顺序见 [`super::chat::wire_chat_domain`]。

use std::rc::Rc;
use std::sync::Arc;

use super::app_shell_ctx::AppShellCtx;
use super::app_shell_wire_phases::run_shell_wiring_in_order;
use super::app_signals::AppSignals;
use super::chat::ChatColumnShell;

/// 执行所有 `wire_*` 注册、闭包构建与 [`AppShellCtx`] 组装（推荐新代码直接调用本函数）。
pub fn bootstrap_app_shell() -> AppShellCtx {
    let app_signals = AppSignals::new();
    let wiring = run_shell_wiring_in_order(&app_signals);

    let new_session = Rc::clone(&wiring.chat_wires.new_session);

    AppShellCtx {
        signals: app_signals.clone(),
        new_session,
        refresh_workspace: Arc::clone(&wiring.refresh_workspace),
        refresh_tasks: Arc::clone(&wiring.refresh_tasks),
        toggle_task: Arc::clone(&wiring.toggle_task),
        refresh_status: Arc::clone(&wiring.refresh_status),
        insert_workspace_file_ref: wiring.insert_workspace_file_ref,
        chat_column: ChatColumnShell {
            app: app_signals,
            stream_shell: wiring.chat_stream_shell.clone(),
            stream_busy_memos: wiring.stream_busy_memos,
            run_send_message: wiring.chat_wires.run_send_message.clone(),
            trigger_stop: Arc::clone(&wiring.chat_wires.cancel_stream),
            regen_stream_after_truncate: wiring.chat_wires.regen_stream_after_truncate,
            retry_assistant_target: wiring.chat_wires.retry_assistant_target,
        },
    }
}
