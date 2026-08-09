//! **阶段 C**：全屏 TUI 内最小对话闭环，复用 [`crate::runtime::cli::repl::repl_dispatch_chat_round`]。
//!
//! 与 REPL 共用配置加载、`CliToolRuntime`、首轮消息准备；**不向 stdout 渲染助手输出**（`suppress_stdout_render`），可按 CLI **`--no-stream`** 选择是否 SSE。
//!
//! **`/` 内建命令**：与 REPL 同源（[`try_handle_repl_slash_command`] + [`repl_slash_handled_followup`]），输出捕获至中区 transcript；**/probe、/models、/mcp** 会短暂退出全屏写 stdout。若配置 **`conversation_store_sqlite_path`**，另有 **`/conv`**、**`/branch`**（与 Web **`conversation_id`** / **`POST /chat/branch`** 同源），并在 **`CM_TUI_CONVERSATION_ID`** 可选指定启动会话 id。
//!
//! 架构：专用线程跑 ratatui + crossterm；[`tokio::sync::mpsc::unbounded_channel`] 投递输入；异步侧执行回合并刷新快照。
//!
//! **焦点**：左（会话）/中上（聊天）/中下（撰写）/右（工作区）四块可点击聚焦（**`EnableMouseCapture`**），标题高亮；**`Tab` / `Shift+Tab`** 循环焦点。面板默认**纯色块、无边框线**（[`panel_chrome`]；**`CM_TUI_PANEL_BG`** 可切 `transparent`/`dim`/`focus`）；**右侧工作区栏聚焦时 `Enter`** 打开工作区 Modal（与 Web 侧栏一致）；**撰写区聚焦时 `Enter`** 提交输入行。字符输入与退格仅在 **「撰写」** 聚焦时生效；
//!
//! **中区 transcript**：与 Web 快照一致的过滤（[`is_message_visible_in_chat_transcript`]）；工具/助手/用户展示路径见 [`transcript`]；本轮旁白/工具序另经 [`turn_project`]（`crabmate-turn-layout` / `project_turn_web_v2`，对齐 Tauri）。回合结束将投影 flush 进 [`transcript::CommittedTurns`]，避免历史退回落盘序。
//!
//! **工具审批**：全屏居中 Modal（↑↓ / jk · Enter · Esc · 1/2/3），与 REPL dialoguer 三项语义一致；不退出 alternate screen。
//!
//! **撰写区**：按单元格宽度自动换行（**`unicode-width`**）；溢出保留底部行；**「撰写」** 聚焦时显示插入光标。
//!
//! **底栏**：对齐 Web / Tauri `status-bar`（左 chips · 右运行态）；快捷键见 `/help`（右栏不再堆快捷键墙）。
//!
//! **聊天区**：溢出时右侧滚动条；可拖动（与滚轮 / PgUp/PgDn 共用 [`TuiModel::chat_scroll_y`]）。
//! 跟底 pin（[`TuiModel::chat_follow_bottom`]）对齐 Web `auto_scroll_chat`：上滑 unpin；
//! 近底 / 下滑回阈值 / 发送 / End re-pin。

mod approval;
mod chat_body;
mod chat_follow;
mod clarify_modal;
mod panel_chrome;
mod poll_loop;
mod refresh;
mod render;
mod session_loop;
mod sidebar_text;
mod sqlite_session;
mod sqlite_slash;
mod sse_mirror;
mod submit_ev;
mod transcript;
mod turn_project;
mod workspace_modal;

mod workspace_switch;

mod model;
mod mouse_key;
mod session_setup;
mod slash_submit;

pub(crate) use model::{
    TuiClarificationShared, TuiFocus, TuiModel, UiEvent, composer_visible_and_cursor_rel,
    compute_tui_pane_layout,
};
pub(crate) use mouse_key::{TuiPollKeyFlow, tui_dispatch_key_press, tui_dispatch_mouse};
pub(crate) use session_setup::tui_header_summary;
pub(crate) use slash_submit::{TuiSlashSubmit, tui_try_consume_slash_submit};

use session_setup::{
    tui_session_build_chrome, tui_session_join_ui_thread, tui_session_new_model,
    tui_session_save_json_session_if_enabled, tui_session_validate_agent_role,
};

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tokio::sync::mpsc::unbounded_channel;

use crate::runtime::cli::{
    CliMainInvocationCommon, ReplSlashSharedHandles, cli_effective_work_dir,
    repl_prepare_messages_and_editor,
};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::tool_approval::TuiApprovalRequest;
use crate::tool_registry::CliToolRuntime;

/// 进入全屏 TUI 并跑对话循环（须 TTY）。**`cli_no_stream`** 对应全局 **`--no-stream`**；助手正文不因流式写入 stdout（保护 alternate screen）。
pub async fn run_tui_session(
    common: CliMainInvocationCommon<'_>,
    cli_no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let CliMainInvocationCommon {
        cfg_holder,
        config_path,
        client,
        api_key,
        tools,
        workspace_cli,
        agent_role,
        process_handles,
    } = common;

    let (run_root, tui_load): (String, bool) = {
        let g = cfg_holder.read().await;
        (
            g.command_exec.run_command_working_dir.clone(),
            g.session_ui.tui_load_session_on_start,
        )
    };
    let mut work_dir = cli_effective_work_dir(workspace_cli, &run_root);
    let (handoff_tx, handoff_rx) = std::sync::mpsc::channel::<TuiTerminalHandoffOp>();
    let (tui_approval_tx, tui_approval_rx) = std::sync::mpsc::sync_channel::<TuiApprovalRequest>(8);
    let cli_rt =
        CliToolRuntime::new_interactive_default().with_tui_blocking_approval(tui_approval_tx);
    let style = CliReplStyle::new();
    let api_key_holder = Arc::new(std::sync::Mutex::new(api_key.to_string()));
    let default_session_mode = {
        let g = cfg_holder.read().await;
        crate::session_mode_turn::resolve_initial_session_mode(&g, agent_role)
    };
    let slash_handles = ReplSlashSharedHandles {
        api_key_holder: Arc::clone(&api_key_holder),
        process_handles: Arc::clone(&process_handles),
        session_mode: Arc::new(std::sync::Mutex::new(default_session_mode)),
    };

    tui_session_validate_agent_role(cfg_holder, agent_role).await?;

    let agent_role_owned = agent_role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let (mut messages, initial_pending, _repl_editor) = repl_prepare_messages_and_editor(
        cfg_holder,
        tui_load,
        work_dir.as_path(),
        &agent_role_owned,
        run_root.as_str(),
        Arc::clone(&process_handles),
    )
    .await?;

    crate::runtime::workspace_session::try_merge_background_initial_workspace(
        &mut messages,
        initial_pending.as_ref(),
    );

    let (sqlite_sess, messages, mut agent_role_owned) =
        sqlite_session::maybe_bootstrap_tui_sqlite(cfg_holder, messages, agent_role_owned).await?;
    let mut sqlite_sess = sqlite_sess;
    let mut messages = messages;

    let chrome = tui_session_build_chrome(
        cfg_holder,
        tui_load,
        work_dir.as_path(),
        &messages,
        &agent_role_owned,
        &sqlite_sess,
    )
    .await;

    let llm_scratch: TuiLlmStreamScratchArc = Arc::new(Mutex::new(TuiLlmStreamScratch::default()));

    let (ev_tx, mut ev_rx) = unbounded_channel::<UiEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let model = tui_session_new_model(chrome, work_dir.clone(), &messages, &sqlite_sess);

    let clarify_shared = TuiClarificationShared {
        inbox: Arc::new(Mutex::new(VecDeque::<
            crate::sse::ClarificationQuestionnaireBody,
        >::new())),
        answers_merge: Arc::new(Mutex::new(
            None::<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
        )),
    };
    let inbox_hook = Arc::clone(&clarify_shared.inbox);
    let model_hook = Arc::clone(&model);
    let clarification_questionnaire_hook: Arc<
        dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync,
    > = Arc::new(move |body| {
        clarify_modal::enqueue_clarification_from_hook(&inbox_hook, &model_hook, body);
    });

    let model_th = Arc::clone(&model);
    let scratch_th = Arc::clone(&llm_scratch);
    let shutdown_th = Arc::clone(&shutdown);
    let clarify_th = clarify_shared.clone();
    let ui_handle: JoinHandle<io::Result<()>> = std::thread::spawn(move || {
        poll_loop::run_tui_ui_thread(
            model_th,
            scratch_th,
            ev_tx,
            shutdown_th,
            tui_approval_rx,
            handoff_rx,
            clarify_th,
        )
    });

    session_loop::run_tui_session_event_loop(session_loop::TuiSessionEventLoopCtx {
        ev_rx: &mut ev_rx,
        clarify_shared: &clarify_shared,
        cfg_holder,
        config_path,
        client,
        tools,
        messages: &mut messages,
        work_dir: &mut work_dir,
        cli_no_stream,
        agent_role_owned: &mut agent_role_owned,
        slash_handles: &slash_handles,
        model: &model,
        handoff_tx: &handoff_tx,
        llm_scratch: &llm_scratch,
        style: &style,
        api_key_holder: &api_key_holder,
        cli_rt: &cli_rt,
        initial_pending: initial_pending.clone(),
        process_handles: Arc::clone(&process_handles),
        clarification_questionnaire_hook: Arc::clone(&clarification_questionnaire_hook),
        sqlite_sess: &mut sqlite_sess,
    })
    .await?;

    tui_session_save_json_session_if_enabled(tui_load, &sqlite_sess, work_dir.as_path(), &messages);

    tui_session_join_ui_thread(shutdown, ui_handle).await?;

    Ok(())
}
