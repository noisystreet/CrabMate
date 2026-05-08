//! App 壳初始化：将所有 wire_* 调用、闭包构建与 AppShellCtx 组装外提，
//! 使 `App` 组件本身仅保留布局组合。
//!
//! # `wire_*` 注册顺序（隐式依赖）
//!
//! 具体分阶段说明见 [`app_shell_wire_phases`]；[`init_app_shell`] 仅按编号调用，**勿**在中间插入其它全局 `Effect`
//! 除非同步更新该模块表格与相关域内文档。
//!
//! 聊天主列**内部**子 `wire_*` 顺序仍见 [`wire_chat_domain`](super::chat::wire_chat_domain)。

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

/// 执行所有 wire_* 注册、闭包构建与 `AppShellCtx` 组装。
pub fn init_app_shell() -> AppShellCtx {
    let app_signals = AppSignals::new();

    wire_phase1_chat_session_lifecycle(&app_signals);
    wire_phase2_persisted_prefs_dom_and_settings_hooks(&app_signals);
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
