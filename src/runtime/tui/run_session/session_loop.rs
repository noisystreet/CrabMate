//! `run_tui_session` 内异步事件循环（拆分以降低单函数 nloc / `mod.rs` 行数）。

use std::sync::{Arc, Mutex as StdMutex};

use reqwest::Client;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::runtime::cli::ReplSlashSharedHandles;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::TuiLlmStreamScratchArc;
use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::tool_registry::CliToolRuntime;
use crate::types::{Message, Tool};

use super::sqlite_session;
use super::submit_ev;
use super::workspace_switch;
use super::{TuiClarificationShared, TuiModel, UiEvent};

pub(super) struct TuiSessionEventLoopCtx<'a> {
    pub(super) ev_rx: &'a mut UnboundedReceiver<UiEvent>,
    pub(super) clarify_shared: &'a TuiClarificationShared,
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) config_path: Option<&'a str>,
    pub(super) client: &'a Client,
    pub(super) tools: &'a [Tool],
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) work_dir: &'a mut std::path::PathBuf,
    pub(super) cli_no_stream: bool,
    pub(super) agent_role_owned: &'a mut Option<String>,
    pub(super) slash_handles: &'a ReplSlashSharedHandles,
    pub(super) model: &'a Arc<StdMutex<TuiModel>>,
    pub(super) handoff_tx: &'a std::sync::mpsc::Sender<TuiTerminalHandoffOp>,
    pub(super) llm_scratch: &'a TuiLlmStreamScratchArc,
    pub(super) style: &'a CliReplStyle,
    pub(super) api_key_holder: &'a Arc<StdMutex<String>>,
    pub(super) cli_rt: &'a CliToolRuntime,
    pub(super) initial_pending: Option<Arc<StdMutex<Option<Vec<Message>>>>>,
    pub(super) process_handles: Arc<ProcessHandles>,
    pub(super) clarification_questionnaire_hook:
        Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>,
    pub(super) sqlite_sess: &'a mut Option<sqlite_session::TuiSqliteSessionState>,
}

pub(super) async fn run_tui_session_event_loop(
    ctx: TuiSessionEventLoopCtx<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    while let Some(ev) = ctx.ev_rx.recv().await {
        match ev {
            UiEvent::Quit => break,
            UiEvent::Submit(input) => {
                let trimmed = input.trim().to_string();
                let allow_empty = ctx
                    .clarify_shared
                    .answers_merge
                    .lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false);
                if trimmed.is_empty() && !allow_empty {
                    continue;
                }
                match submit_ev::tui_run_submit_ev(
                    trimmed,
                    submit_ev::TuiSubmitEv {
                        clarify_shared: ctx.clarify_shared,
                        cfg_holder: ctx.cfg_holder,
                        config_path: ctx.config_path,
                        client: ctx.client,
                        tools: ctx.tools,
                        messages: ctx.messages,
                        work_dir: ctx.work_dir,
                        cli_no_stream: ctx.cli_no_stream,
                        agent_role_owned: ctx.agent_role_owned,
                        slash_handles: ctx.slash_handles,
                        model: ctx.model,
                        handoff_tx: ctx.handoff_tx,
                        llm_scratch: ctx.llm_scratch,
                        style: ctx.style,
                        api_key_holder: ctx.api_key_holder,
                        cli_rt: ctx.cli_rt,
                        initial_pending: ctx.initial_pending.clone(),
                        process_handles: Arc::clone(&ctx.process_handles),
                        clarification_questionnaire_hook: Arc::clone(
                            &ctx.clarification_questionnaire_hook,
                        ),
                        sqlite_session: ctx.sqlite_sess.as_mut(),
                    },
                )
                .await?
                {
                    submit_ev::TuiSubmitHandled::SlashOnly => continue,
                    submit_ev::TuiSubmitHandled::RanRound => {}
                }
            }
            UiEvent::WorkspaceSwitch(raw) => {
                workspace_switch::tui_event_workspace_switch(
                    raw,
                    workspace_switch::TuiWorkspaceUiSwitch {
                        cfg_holder: ctx.cfg_holder,
                        work_dir: ctx.work_dir,
                        model: ctx.model,
                        agent_role_owned: ctx.agent_role_owned,
                        message_count: ctx.messages.len(),
                        tool_count: ctx.tools.len(),
                        cli_no_stream: ctx.cli_no_stream,
                        process_handles: &ctx.process_handles,
                    },
                )
                .await;
            }
        }
    }
    Ok(())
}
