//! TUI 专用斜杠包装：调用共享 [`crate::runtime::cli_sqlite_slash`]。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::runtime::cli_sqlite_slash::{CliSqliteSlashResult, try_apply_cli_sqlite_slash};
use crate::runtime::workspace_session;
use crate::tool_stats::ToolOutcomeRecorder;
use crate::types::Message;

use super::TuiModel;
use super::refresh::{TuiAfterChatRoundRefresh, tui_refresh_after_chat_round};
use super::sqlite_session::TuiSqliteSessionState;

pub(super) struct TuiSqliteSlashEnv<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) work_dir: &'a std::path::Path,
    pub(super) tool_count: usize,
    pub(super) cli_no_stream: bool,
    pub(super) process_handles: &'a Arc<ProcessHandles>,
}

fn push_block(model: &Arc<Mutex<TuiModel>>, lines: &[String]) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.transcript.push_str("\n[/conv]\n");
    for ln in lines {
        g.transcript.push_str(ln);
        g.transcript.push('\n');
    }
    g.chat_snap_bottom_next_draw = true;
    g.chat_follow_bottom = true;
}

async fn tui_sqlite_slash_refresh_ui(
    env: &TuiSqliteSlashEnv<'_>,
    messages: &[Message],
    agent_role_owned: &Option<String>,
) {
    tui_refresh_after_chat_round(TuiAfterChatRoundRefresh {
        model: env.model,
        cfg_holder: env.cfg_holder,
        work_dir: env.work_dir,
        agent_role_owned,
        messages,
        tool_count: env.tool_count,
        cli_no_stream: env.cli_no_stream,
        sqlite_persist: None,
        process_handles: env.process_handles,
    })
    .await;
}

pub(super) async fn tui_try_consume_sqlite_slash(
    trimmed: &str,
    sqlite_slot: &mut Option<&mut TuiSqliteSessionState>,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
    env: &TuiSqliteSlashEnv<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let needs_bootstrap = trimmed.starts_with("/conv")
        && trimmed
            .split_whitespace()
            .nth(1)
            .is_some_and(|s| s == "new");
    let bootstrap = if needs_bootstrap {
        let cfg = env.cfg_holder.read().await;
        let rec = Arc::new(ToolOutcomeRecorder::new());
        Some(workspace_session::repl_bootstrap_messages_fast(
            &cfg,
            agent_role_owned.as_ref().map(|s| s.as_str()),
            &rec,
        ))
    } else {
        None
    };

    let result = try_apply_cli_sqlite_slash(
        trimmed,
        sqlite_slot.as_deref_mut(),
        messages,
        agent_role_owned,
        bootstrap,
    );
    match result {
        CliSqliteSlashResult::NotHandled => Ok(false),
        CliSqliteSlashResult::Handled { lines } => {
            push_block(env.model, &lines);
            let refresh = trimmed.starts_with("/branch")
                || trimmed
                    .split_whitespace()
                    .nth(1)
                    .is_some_and(|s| matches!(s, "open" | "new"));
            if refresh {
                if let Some(sess) = sqlite_slot.as_ref() {
                    let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
                    g.sqlite_conversation_id = Some(sess.conversation_id.clone());
                }
                tui_sqlite_slash_refresh_ui(env, messages.as_slice(), agent_role_owned).await;
            } else if trimmed.starts_with("/conv") || trimmed.starts_with("/branch") {
                let chips = super::sidebar_text::tui_status_chips_line_with_messages(
                    env.cfg_holder,
                    agent_role_owned,
                    messages,
                )
                .await;
                let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
                g.status_chips = format!("{chips} · /conv /branch");
            }
            Ok(true)
        }
    }
}
