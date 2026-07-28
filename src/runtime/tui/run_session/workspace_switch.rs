//! TUI 工作区切换：异步侧应用路径并与 Web **`POST /workspace`** / REPL **`/workspace`** 对齐。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::runtime::workspace_session;

use super::{TuiModel, tui_header_summary};

pub(super) struct TuiWorkspaceUiSwitch<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) agent_role_owned: &'a Option<String>,
    pub(super) message_count: usize,
}

pub(super) async fn tui_event_workspace_switch(raw: String, ctx: TuiWorkspaceUiSwitch<'_>) {
    let TuiWorkspaceUiSwitch {
        cfg_holder,
        work_dir,
        model,
        agent_role_owned,
        message_count,
    } = ctx;
    if let Err(msg) = tui_apply_workspace_switch(
        raw,
        TuiWorkspaceApplyParams {
            cfg_holder,
            work_dir,
            model,
            agent_role_owned,
            message_count,
        },
    )
    .await
    {
        let chips = super::sidebar_text::tui_status_chips_line(cfg_holder, agent_role_owned).await;
        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.status_chips = format!("{chips} · 工作区: {msg}");
        g.status_run = super::sidebar_text::tui_status_run_ready().to_string();
    }
}

pub(super) struct TuiWorkspaceApplyParams<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) agent_role_owned: &'a Option<String>,
    pub(super) message_count: usize,
}

pub(super) async fn tui_apply_workspace_switch(
    raw: String,
    p: TuiWorkspaceApplyParams<'_>,
) -> Result<(), String> {
    let TuiWorkspaceApplyParams {
        cfg_holder,
        work_dir,
        model,
        agent_role_owned,
        message_count,
    } = p;
    let new_root = {
        let cfg = cfg_holder.read().await;
        crate::tools::resolve_repl_workspace_switch_path(&cfg, work_dir.as_path(), raw.as_str())
            .map_err(|e| e.to_string())?
    };
    *work_dir = new_root;
    let new_header = tui_header_summary(work_dir.as_path());
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let (sqlite_nav, recent_ids) = {
        let g = model.lock().unwrap_or_else(|e| e.into_inner());
        (
            g.sqlite_conversation_id.clone(),
            g.recent_conversation_ids.clone(),
        )
    };
    let nav = super::sidebar_text::build_tui_session_sidebar(
        tui_load_nav,
        workspace_session::session_file_path(work_dir.as_path()).exists(),
        message_count,
        sqlite_nav.as_deref(),
        &recent_ids,
    );
    let right = super::sidebar_text::build_tui_workspace_sidebar(work_dir.as_path());
    let chips = super::sidebar_text::tui_status_chips_line(cfg_holder, agent_role_owned).await;
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.workspace_path_buf = work_dir.clone();
    g.status_chips = chips;
    g.status_run = super::sidebar_text::tui_status_run_ready().to_string();
    Ok(())
}
