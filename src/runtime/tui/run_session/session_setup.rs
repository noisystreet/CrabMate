//! TUI 会话启动：chrome 构建、模型初始化与 UI 线程 join。

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::config::SharedAgentConfig;
use crate::runtime::cli_exit::{CliExitError, EXIT_USAGE};
use crate::runtime::workspace_session;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::Message;

use super::model::TuiModel;
use super::sidebar_text;
use super::transcript;
use super::turn_project;

pub(crate) fn tui_header_summary(work_dir: &std::path::Path) -> String {
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 72);
    format!("CrabMate · {wd_short}")
}

pub(super) async fn tui_session_validate_agent_role(
    cfg_holder: &SharedAgentConfig,
    agent_role: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let g = cfg_holder.read().await;
    if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
        g.system_prompt_for_new_conversation(Some(r))
            .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
    }
    Ok(())
}

pub(super) struct TuiSessionChrome {
    header_line: String,
    nav_summary: String,
    right_summary: String,
    status_chips: String,
    recent_conversations: Vec<crate::conversation_store::ConversationListEntry>,
}

pub(super) async fn tui_session_build_chrome(
    cfg_holder: &SharedAgentConfig,
    tui_load: bool,
    work_dir: &std::path::Path,
    messages: &[Message],
    agent_role_owned: &Option<String>,
    sqlite_sess: &Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
) -> TuiSessionChrome {
    let sqlite_id_nav = sqlite_sess.as_ref().map(|s| s.conversation_id.as_str());
    let recent_conversations = sidebar_text::tui_recent_conversations(sqlite_sess.as_ref());
    TuiSessionChrome {
        header_line: tui_header_summary(work_dir),
        nav_summary: sidebar_text::build_tui_session_sidebar(
            tui_load,
            workspace_session::session_file_path(work_dir).exists(),
            messages.len(),
            sqlite_id_nav,
            &recent_conversations,
        ),
        right_summary: sidebar_text::build_tui_workspace_sidebar(work_dir),
        status_chips: sidebar_text::tui_status_chips_line_with_messages(
            cfg_holder,
            agent_role_owned,
            messages,
        )
        .await,
        recent_conversations,
    }
}

pub(super) fn tui_session_new_model(
    chrome: TuiSessionChrome,
    work_dir: std::path::PathBuf,
    messages: &[Message],
    sqlite_sess: &Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
) -> Arc<Mutex<TuiModel>> {
    let committed_turns = transcript::CommittedTurns::reseed_from_messages(messages);
    Arc::new(Mutex::new(TuiModel {
        header_line: chrome.header_line,
        nav_summary: chrome.nav_summary,
        right_summary: chrome.right_summary,
        transcript: committed_turns.display.clone(),
        chat_scroll_y: 0,
        chat_snap_bottom_next_draw: false,
        chat_follow_bottom: true,
        chat_user_scroll_down: false,
        chat_scrollbar_dragging: false,
        input: String::new(),
        status_chips: chrome.status_chips,
        status_run: sidebar_text::tui_status_run_ready().to_string(),
        focus: super::model::TuiFocus::default(),
        approval_modal: None,
        approval_backlog: VecDeque::new(),
        clarification_modal: None,
        clarification_backlog: VecDeque::new(),
        workspace_path_buf: work_dir,
        workspace_modal: None,
        sqlite_conversation_id: sqlite_sess.as_ref().map(|s| s.conversation_id.clone()),
        recent_conversations: chrome.recent_conversations,
        control_plane_tail: String::new(),
        turn_projection: turn_project::TuiTurnProjection::default(),
        committed_turns,
    }))
}

pub(super) fn tui_session_save_json_session_if_enabled(
    tui_load: bool,
    sqlite_sess: &Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
    work_dir: &std::path::Path,
    messages: &[Message],
) {
    if tui_load
        && sqlite_sess.is_none()
        && let Err(e) = workspace_session::save_workspace_session(work_dir, messages)
    {
        eprintln!(
            "写入 {} 失败: {e}",
            workspace_session::session_file_path(work_dir).display()
        );
    }
}

pub(super) async fn tui_session_join_ui_thread(
    shutdown: Arc<AtomicBool>,
    ui_handle: JoinHandle<io::Result<()>>,
) -> Result<(), Box<dyn std::error::Error>> {
    shutdown.store(true, Ordering::SeqCst);
    let join_out = tokio::task::spawn_blocking(move || ui_handle.join())
        .await
        .map_err(|e| io::Error::other(format!("join tui task: {e:?}")))?
        .map_err(|e| io::Error::other(format!("tui thread join: {e:?}")))?;
    join_out?;
    Ok(())
}
