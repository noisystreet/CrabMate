//! [`UiEvent::Submit`]：可选澄清空提交、`/` 斜杠命令、`repl_dispatch_chat_round` 与会话刷新。

use std::sync::Arc;

use reqwest::Client;

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::runtime::cli::{
    ReplAfterUserMessageEnqueuedCb, ReplDispatchChatRoundParams, ReplSlashSharedHandles,
    repl_dispatch_chat_round,
};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::TuiLlmStreamScratchArc;
use crate::tool_registry::CliToolRuntime;
use crate::types::{Message, Tool};

use super::refresh::{TuiAfterChatRoundRefresh, tui_refresh_after_chat_round};
use super::{
    TuiClarificationShared, TuiModel, TuiSlashSubmit, sidebar_text,
    sqlite_slash::{TuiSqliteSlashEnv, tui_try_consume_sqlite_slash},
    transcript, tui_try_consume_slash_submit,
};

fn tui_make_submit_hooks(
    model: &Arc<std::sync::Mutex<TuiModel>>,
) -> (
    ReplAfterUserMessageEnqueuedCb,
    Arc<dyn Fn(bool) + Send + Sync>,
) {
    let model_refresh = Arc::clone(model);
    let on_user_enqueued: ReplAfterUserMessageEnqueuedCb = Arc::new(move |msgs: &[Message]| {
        let mut g = model_refresh.lock().unwrap_or_else(|e| e.into_inner());
        g.transcript = transcript::transcript_with_in_progress(&g.committed_turns, msgs);
        // 对齐 Web `engage_follow_and_scroll_bottom`：用户气泡写入后再 snap（Enter 时的下一帧
        // 往往早于本回调，仅靠那一次 snap 会贴在旧高度上）。
        g.chat_follow_bottom = true;
        g.chat_snap_bottom_next_draw = true;
        g.status_run = sidebar_text::tui_status_run_model_busy().to_string();
    });
    let model_for_hook = Arc::clone(model);
    let tool_running_hook: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |running| {
        let mut g = model_for_hook.lock().unwrap_or_else(|e| e.into_inner());
        g.status_run = if running {
            sidebar_text::tui_status_run_tool_busy().to_string()
        } else {
            sidebar_text::tui_status_run_model_busy().to_string()
        };
    });
    (on_user_enqueued, tool_running_hook)
}

pub(super) enum TuiSubmitHandled {
    /// `try_handle_repl_slash_command` 已处理当前输入；不应再走对话轮。
    SlashOnly,
    /// 已完成一轮对话并刷新 UI。
    RanRound,
}

pub(super) struct TuiSubmitEv<'a> {
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
    pub(super) model: &'a Arc<std::sync::Mutex<TuiModel>>,
    pub(super) handoff_tx:
        &'a std::sync::mpsc::Sender<crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp>,
    pub(super) llm_scratch: &'a TuiLlmStreamScratchArc,
    pub(super) style: &'a CliReplStyle,
    pub(super) api_key_holder: &'a Arc<std::sync::Mutex<String>>,
    pub(super) cli_rt: &'a CliToolRuntime,
    pub(super) initial_pending: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<Message>>>>>,
    pub(super) process_handles: Arc<ProcessHandles>,
    pub(super) clarification_questionnaire_hook:
        std::sync::Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>,
    pub(super) sqlite_session: Option<&'a mut super::sqlite_session::TuiSqliteSessionState>,
}

pub(super) async fn tui_run_submit_ev(
    trimmed: String,
    mut ctx: TuiSubmitEv<'_>,
) -> Result<TuiSubmitHandled, Box<dyn std::error::Error>> {
    if tui_try_consume_sqlite_slash(
        trimmed.as_str(),
        &mut ctx.sqlite_session,
        ctx.messages,
        ctx.agent_role_owned,
        &TuiSqliteSlashEnv {
            cfg_holder: ctx.cfg_holder,
            model: ctx.model,
            work_dir: ctx.work_dir.as_path(),
        },
    )
    .await?
    {
        return Ok(TuiSubmitHandled::SlashOnly);
    }

    if tui_try_consume_slash_submit(
        trimmed.as_str(),
        TuiSlashSubmit {
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
        },
    )
    .await?
    {
        return Ok(TuiSubmitHandled::SlashOnly);
    }
    {
        let mut s = ctx.llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
        s.clear();
    }
    {
        let mut m = ctx.model.lock().unwrap_or_else(|e| e.into_inner());
        m.control_plane_tail.clear();
        m.turn_projection.reset();
    }
    let sse_mirror_hook = super::sse_mirror::tui_sse_control_mirror(
        Arc::clone(ctx.model),
        Arc::clone(ctx.llm_scratch),
    );
    let (on_user_enqueued, tool_running_hook) = tui_make_submit_hooks(ctx.model);
    repl_dispatch_chat_round(ReplDispatchChatRoundParams {
        input: trimmed,
        cfg_holder: ctx.cfg_holder,
        tools: ctx.tools,
        messages: ctx.messages,
        work_dir: ctx.work_dir,
        style: ctx.style,
        no_stream: ctx.cli_no_stream,
        suppress_stdout_render: true,
        tui_llm_stream_scratch: Some(Arc::clone(ctx.llm_scratch)),
        tool_running_hook: Some(tool_running_hook),
        after_user_message_enqueued: Some(on_user_enqueued),
        agent_role_owned: ctx.agent_role_owned,
        api_key_holder: ctx.api_key_holder,
        client: ctx.client,
        cli_rt: ctx.cli_rt,
        initial_pending: ctx.initial_pending.as_ref(),
        process_handles: Arc::clone(&ctx.process_handles),
        clarify_answers_for_next_user_message: Some(&ctx.clarify_shared.answers_merge),
        clarification_questionnaire_hook: Some(Arc::clone(&ctx.clarification_questionnaire_hook)),
        sse_control_mirror: Some(sse_mirror_hook),
    })
    .await?;
    {
        let mut s = ctx.llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut m = ctx.model.lock().unwrap_or_else(|e| e.into_inner());
            m.turn_projection.finalize_for_display(&s);
            let projection_snapshot = std::mem::take(&mut m.turn_projection);
            m.committed_turns
                .flush_completed_turn(ctx.messages.as_slice(), &projection_snapshot);
            // `take` 已清空本轮投影；再显式 default 以对齐 reset 语义
            m.turn_projection = Default::default();
            m.control_plane_tail.clear();
        }
        s.clear();
    }
    tui_refresh_after_chat_round(TuiAfterChatRoundRefresh {
        model: ctx.model,
        cfg_holder: ctx.cfg_holder,
        work_dir: ctx.work_dir.as_path(),
        agent_role_owned: ctx.agent_role_owned,
        messages: ctx.messages.as_slice(),
        sqlite_persist: Some(&mut ctx.sqlite_session),
    })
    .await;
    Ok(TuiSubmitHandled::RanRound)
}
