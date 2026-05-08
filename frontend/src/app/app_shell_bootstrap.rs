//! App 壳层接线的**单一入口**：[`bootstrap_app_shell`] 依序调用各阶段 `wire_*`，
//! 避免在 `App` 或其它处零散插入导致顺序错误。阶段语义与编号见
//! [`super::app_shell_wire_phases`]；聊天主列内部顺序见 [`super::chat::wire_chat_domain`]。

use std::rc::Rc;
use std::sync::Arc;

use super::app_shell_ctx::AppShellCtx;
use super::app_shell_wire_phases::{
    make_refresh_workspace_for_shell, wire_phase1_chat_session_lifecycle,
    wire_phase2_persisted_prefs_dom_and_settings_hooks, wire_phase3_escape_layered_dismiss,
    wire_phase4_workspace_status_and_chat_domain,
};
use super::app_signals::AppSignals;
use super::chat::ChatColumnShell;

/// 执行所有 `wire_*` 注册、闭包构建与 [`AppShellCtx`] 组装（推荐新代码直接调用本函数）。
pub fn bootstrap_app_shell() -> AppShellCtx {
    let app_signals = AppSignals::new();

    // Phase 1 — 会话生命周期（须最先）。
    wire_phase1_chat_session_lifecycle(&app_signals);
    // Phase 2 — 本机偏好、DOM、设置 LLM 草稿挂钩。
    wire_phase2_persisted_prefs_dom_and_settings_hooks(&app_signals);
    // Phase 3 — Escape 分层关闭（依赖阶段 2 已挂接的弹层信号）。
    wire_phase3_escape_layered_dismiss(&app_signals);

    let refresh_workspace = make_refresh_workspace_for_shell(&app_signals);

    let (
        refresh_status,
        refresh_tasks,
        toggle_task,
        insert_workspace_file_ref_sv,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    ) = wire_phase4_workspace_status_and_chat_domain(&app_signals, Arc::clone(&refresh_workspace));

    let new_session = Rc::clone(&chat_wires.new_session);

    AppShellCtx {
        signals: app_signals.clone(),
        new_session,
        refresh_workspace: Arc::clone(&refresh_workspace),
        refresh_tasks: Arc::clone(&refresh_tasks),
        toggle_task: Arc::clone(&toggle_task),
        refresh_status: Arc::clone(&refresh_status),
        insert_workspace_file_ref: insert_workspace_file_ref_sv,
        chat_column: ChatColumnShell {
            app: app_signals,
            stream_shell: chat_stream_shell.clone(),
            stream_busy_memos,
            run_send_message: chat_wires.run_send_message.clone(),
            trigger_stop: Arc::clone(&chat_wires.cancel_stream),
            regen_stream_after_truncate: chat_wires.regen_stream_after_truncate,
            retry_assistant_target: chat_wires.retry_assistant_target,
        },
    }
}
