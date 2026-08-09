//! TUI `/…` 斜杠命令提交与 transcript 刷新。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::runtime::cli::{
    ReplSlashFollowupCtx, ReplSlashHandled, ReplSlashSharedHandles, repl_slash_handled_followup,
    try_handle_repl_slash_command,
};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::runtime::workspace_session;
use crate::types::Message;

use super::model::TuiModel;
use super::session_setup::tui_header_summary;
use super::sidebar_text;

struct TuiSlashUiRefresh<'a> {
    model: &'a Arc<Mutex<TuiModel>>,
    cfg_holder: &'a SharedAgentConfig,
    work_dir: &'a std::path::Path,
    agent_role_owned: &'a Option<String>,
    message_count: usize,
    captured: Vec<String>,
}

pub(crate) struct TuiSlashSubmit<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) config_path: Option<&'a str>,
    pub(super) client: &'a reqwest::Client,
    pub(super) tools: &'a [crate::types::Tool],
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) cli_no_stream: bool,
    pub(super) agent_role_owned: &'a mut Option<String>,
    pub(super) slash_handles: &'a ReplSlashSharedHandles,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) handoff_tx: &'a std::sync::mpsc::Sender<TuiTerminalHandoffOp>,
}

pub(crate) async fn tui_try_consume_slash_submit(
    trimmed: &str,
    ctx: TuiSlashSubmit<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !trimmed.starts_with('/') {
        return Ok(false);
    }
    let cap = Arc::new(Mutex::new(Vec::<String>::new()));
    let style_cap = CliReplStyle::new_tui_capture(Arc::clone(&cap));
    let handled = try_handle_repl_slash_command(
        trimmed,
        ctx.cfg_holder,
        ctx.tools,
        ctx.messages,
        ctx.work_dir,
        &style_cap,
        ctx.cli_no_stream,
        ctx.agent_role_owned,
        ctx.slash_handles,
    )
    .await;
    if matches!(handled, ReplSlashHandled::NotSlash) {
        let mut g = ctx.model.lock().unwrap_or_else(|e| e.into_inner());
        g.status_run = sidebar_text::tui_status_run_error(
            "输入以 / 开头但未识别为内建命令（不应发生）；请报告 issue",
        );
        return Ok(true);
    }
    repl_slash_handled_followup(
        handled,
        ReplSlashFollowupCtx {
            cfg_holder: ctx.cfg_holder,
            config_path: ctx.config_path,
            client: ctx.client,
            slash_handles: ctx.slash_handles,
            style: &style_cap,
            work_dir: ctx.work_dir.as_path(),
            tui_terminal_tx: Some(ctx.handoff_tx),
        },
    )
    .await?;
    let captured = cap.lock().unwrap_or_else(|e| e.into_inner()).clone();
    tui_refresh_after_slash_capture(TuiSlashUiRefresh {
        model: ctx.model,
        cfg_holder: ctx.cfg_holder,
        work_dir: ctx.work_dir.as_path(),
        agent_role_owned: ctx.agent_role_owned,
        message_count: ctx.messages.len(),
        captured,
    })
    .await;
    Ok(true)
}

async fn tui_refresh_after_slash_capture(p: TuiSlashUiRefresh<'_>) {
    let TuiSlashUiRefresh {
        model,
        cfg_holder,
        work_dir,
        agent_role_owned,
        message_count,
        captured,
    } = p;
    let new_header = tui_header_summary(work_dir);
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let (sqlite_nav, recent_ids) = {
        let g = model.lock().unwrap_or_else(|e| e.into_inner());
        (
            g.sqlite_conversation_id.as_deref().map(|s| s.to_string()),
            g.recent_conversations.clone(),
        )
    };
    let nav = sidebar_text::build_tui_session_sidebar(
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        message_count,
        sqlite_nav.as_deref(),
        &recent_ids,
    );
    let right = sidebar_text::build_tui_workspace_sidebar(work_dir);
    let chips = sidebar_text::tui_status_chips_line(cfg_holder, agent_role_owned).await;

    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    if !captured.is_empty() {
        g.transcript.push_str("\n[/]\n");
        for ln in captured {
            g.transcript.push_str(&ln);
            g.transcript.push('\n');
        }
        g.transcript.push('\n');
    }
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.workspace_path_buf = work_dir.to_path_buf();
    g.status_chips = chips;
    g.status_run = sidebar_text::tui_status_run_ready().to_string();
}
