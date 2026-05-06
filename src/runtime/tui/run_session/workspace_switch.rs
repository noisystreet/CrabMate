//! TUI 工作区切换：异步侧应用路径并与 Web **`POST /workspace`** / REPL **`/workspace`** 对齐。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::runtime::workspace_session;

use super::{
    TuiModel, build_tui_nav_summary, build_tui_right_summary, tui_header_summary,
    tui_status_bar_with_run, tui_status_chips_line,
};

pub(super) struct TuiWorkspaceUiSwitch<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) agent_role_owned: &'a Option<String>,
    pub(super) message_count: usize,
    pub(super) tool_count: usize,
    pub(super) cli_no_stream: bool,
}

pub(super) async fn tui_event_workspace_switch(raw: String, ctx: TuiWorkspaceUiSwitch<'_>) {
    let TuiWorkspaceUiSwitch {
        cfg_holder,
        work_dir,
        model,
        agent_role_owned,
        message_count,
        tool_count,
        cli_no_stream,
    } = ctx;
    if let Err(msg) = tui_apply_workspace_switch(
        raw,
        TuiWorkspaceApplyParams {
            cfg_holder,
            work_dir,
            model,
            agent_role_owned,
            message_count,
            tool_count,
            cli_no_stream,
        },
    )
    .await
    {
        let chips = tui_status_chips_line(cfg_holder, agent_role_owned).await;
        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.status = format!("{} · 工作区: {}", chips, msg);
    }
}

pub(super) struct TuiWorkspaceApplyParams<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) agent_role_owned: &'a Option<String>,
    pub(super) message_count: usize,
    pub(super) tool_count: usize,
    pub(super) cli_no_stream: bool,
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
        tool_count,
        cli_no_stream,
    } = p;
    let new_root = {
        let cfg = cfg_holder.read().await;
        crate::tools::resolve_repl_workspace_switch_path(&cfg, work_dir.as_path(), raw.as_str())
            .map_err(|e| e.to_string())?
    };
    *work_dir = new_root;
    let new_header = tui_header_summary(cfg_holder, work_dir.as_path()).await;
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let nav = build_tui_nav_summary(
        work_dir.as_path(),
        tui_load_nav,
        workspace_session::session_file_path(work_dir.as_path()).exists(),
        message_count,
    );
    let right = build_tui_right_summary(tool_count, cli_no_stream);
    let chips = tui_status_chips_line(cfg_holder, agent_role_owned).await;
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.workspace_path_buf = work_dir.clone();
    g.status_chips = chips.clone();
    g.status = tui_status_bar_with_run(&chips, "就绪");
    Ok(())
}
