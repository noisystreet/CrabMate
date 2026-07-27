//! 对话回合结束后刷新侧栏与中区 transcript（含 [`super::transcript::CommittedTurns`] 同步）。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::runtime::workspace_session;
use crate::types::Message;

use super::sidebar_text;
use super::sqlite_session;
use super::workspace_sidebar_extra;
use super::{TuiModel, tui_header_summary};

pub(super) struct TuiAfterChatRoundRefresh<'a> {
    pub model: &'a Arc<Mutex<TuiModel>>,
    pub cfg_holder: &'a SharedAgentConfig,
    pub work_dir: &'a std::path::Path,
    pub agent_role_owned: &'a Option<String>,
    pub messages: &'a [Message],
    pub tool_count: usize,
    pub cli_no_stream: bool,
    pub sqlite_persist: Option<&'a mut Option<&'a mut sqlite_session::TuiSqliteSessionState>>,
    pub process_handles: &'a Arc<ProcessHandles>,
}

pub(super) async fn tui_refresh_after_chat_round(p: TuiAfterChatRoundRefresh<'_>) {
    let TuiAfterChatRoundRefresh {
        model,
        cfg_holder,
        work_dir,
        agent_role_owned,
        messages,
        tool_count,
        cli_no_stream,
        sqlite_persist,
        process_handles,
    } = p;
    let persist_note = if let Some(sqlite_slot) = sqlite_persist
        && let Some(sess) = sqlite_slot.as_mut()
    {
        (*sess)
            .persist_round(messages, agent_role_owned.as_deref())
            .err()
    } else {
        None
    };
    let new_header = tui_header_summary(work_dir);
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let sqlite_nav = {
        let g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.sqlite_conversation_id.clone()
    };
    let nav = sidebar_text::build_tui_session_sidebar(
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        messages.len(),
        sqlite_nav.as_deref(),
    );
    let right = workspace_sidebar_extra::build_tui_workspace_sidebar_extended(
        work_dir,
        tool_count,
        cli_no_stream,
        process_handles,
        cfg_holder,
        sqlite_nav.as_deref(),
    )
    .await;
    let chips =
        sidebar_text::tui_status_chips_line_with_messages(cfg_holder, agent_role_owned, messages)
            .await;

    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let session_replaced = g.committed_turns.msg_len != messages.len();
    g.committed_turns.ensure_consistent_with(messages);
    if session_replaced {
        g.turn_projection.reset();
        g.control_plane_tail.clear();
    }
    g.transcript = g.committed_turns.display.clone();
    g.chat_snap_bottom_next_draw = true;
    g.chat_follow_bottom = true;
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.workspace_path_buf = work_dir.to_path_buf();
    g.status_chips = chips;
    g.status_run = match persist_note {
        Some(err) => sidebar_text::tui_status_run_error(&format!("SQLite: {err}")),
        None => sidebar_text::tui_status_run_ready().to_string(),
    };
}
