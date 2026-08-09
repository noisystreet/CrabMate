//! REPL 主循环单次迭代：斜杠分支、本地 shell、普通对话回合。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::ProcessHandles;
use crate::config::SharedAgentConfig;
use crate::runtime::cli::repl_extras::{
    ReplSlashHandled, ReplSlashSharedHandles, try_handle_repl_slash_command,
};
use crate::runtime::cli::repl_parse::run_repl_shell_line_sync;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::repl_reedline::ReplReadLine;
use crate::tool_registry::CliToolRuntime;
use crate::types::Message;

use super::repl_chat_round::{ReplDispatchChatRoundParams, repl_dispatch_chat_round};
use super::repl_slash_followup::{ReplSlashFollowupCtx, repl_slash_handled_followup};

const REPL_SHELL_USAGE: &str = "bash#: <命令>  在当前工作区执行一行 shell（不发给模型；无交互 stdin）。等同本机 `sh -c` / `cmd /C`，不受模型 `run_command` 白名单约束，仅应在可信环境使用。交互 TTY：空行按 `$` 即切换「我:」/ bash#:（也可单独一行 `$` 后 Enter）；管道/非 TTY 仍可用行内 `$ <命令>`。历史保存在工作区 `.crabmate/repl_history.txt`。示例: ls  pwd  git status";

/// 处理 `ReplReadLine::Chat` 中的 `/` 命令分支：`true` 表示已消费输入并应 `continue` 主循环；`false` 表示继续走普通对话回合。
struct ReplSlashBranchContinueLoopParams<'a> {
    input: &'a str,
    cfg_holder: &'a SharedAgentConfig,
    config_path: Option<&'a str>,
    tools: &'a [crate::types::Tool],
    messages: &'a mut Vec<Message>,
    work_dir: &'a mut PathBuf,
    style: &'a CliReplStyle,
    no_stream: bool,
    agent_role_owned: &'a mut Option<String>,
    slash_handles: &'a ReplSlashSharedHandles,
    client: &'a reqwest::Client,
    sqlite_sess: &'a mut Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
}

async fn repl_slash_branch_continue_loop(
    p: ReplSlashBranchContinueLoopParams<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let ReplSlashBranchContinueLoopParams {
        input,
        cfg_holder,
        config_path,
        tools,
        messages,
        work_dir,
        style,
        no_stream,
        agent_role_owned,
        slash_handles,
        client,
        sqlite_sess,
    } = p;

    let needs_bootstrap = input.trim().starts_with("/conv")
        && input.split_whitespace().nth(1).is_some_and(|s| s == "new");
    let bootstrap = if needs_bootstrap {
        let cfg = cfg_holder.read().await;
        Some(
            crate::runtime::workspace_session::repl_bootstrap_messages_fast(
                &cfg,
                agent_role_owned.as_deref(),
                &slash_handles.process_handles.tool_outcome_recorder,
            ),
        )
    } else {
        None
    };
    match crate::runtime::cli_sqlite_slash::try_apply_cli_sqlite_slash(
        input.trim(),
        sqlite_sess.as_mut(),
        messages,
        agent_role_owned,
        bootstrap,
    ) {
        crate::runtime::cli_sqlite_slash::CliSqliteSlashResult::NotHandled => {}
        crate::runtime::cli_sqlite_slash::CliSqliteSlashResult::Handled { lines } => {
            for ln in lines {
                let _ = style.print_line(&ln);
            }
            return Ok(true);
        }
    }

    let handled = try_handle_repl_slash_command(
        input,
        cfg_holder,
        tools,
        messages,
        work_dir,
        style,
        no_stream,
        agent_role_owned,
        slash_handles,
    )
    .await;
    if matches!(handled, ReplSlashHandled::NotSlash) {
        return Ok(false);
    }
    repl_slash_handled_followup(
        handled,
        ReplSlashFollowupCtx {
            cfg_holder,
            config_path,
            client,
            slash_handles,
            style,
            work_dir: work_dir.as_path(),
            tui_terminal_tx: None,
        },
    )
    .await?;
    Ok(true)
}

/// 执行 REPL 本地 shell 一行：`parsed` 为 `repl_reedline::parse_repl_dollar_shell_line` 的 `Some(...)` 内层；`None` 表示仅 `$` 或空命令，打印用法。
fn repl_execute_shell(
    parsed: Option<&str>,
    work_dir: &Path,
    style: &CliReplStyle,
) -> io::Result<()> {
    let cmd = match parsed {
        None => None,
        Some(c) => {
            let t = c.trim();
            if t.is_empty() { None } else { Some(t) }
        }
    };
    let Some(cmd) = cmd else {
        let _ = style.print_line(REPL_SHELL_USAGE);
        return Ok(());
    };
    if cmd.contains('\0') {
        let _ = style.eprint_error("命令含空字节，已拒绝执行。");
        return Ok(());
    }
    let code = run_repl_shell_line_sync(cmd, work_dir)?;
    if code != 0 {
        let _ = style.print_line(&format!("退出码: {code}"));
    }
    Ok(())
}

pub(super) enum ReplMainIterationCtl {
    BreakRepl,
    Continue,
}

pub(super) struct ReplIterationCtx<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) config_path: Option<&'a str>,
    pub(super) client: &'a reqwest::Client,
    pub(super) tools: &'a [crate::types::Tool],
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) work_dir: &'a mut PathBuf,
    pub(super) style: &'a CliReplStyle,
    pub(super) no_stream: bool,
    pub(super) agent_role_owned: &'a mut Option<String>,
    pub(super) slash_handles: &'a ReplSlashSharedHandles,
    pub(super) api_key_holder: &'a Arc<StdMutex<String>>,
    pub(super) cli_rt: &'a CliToolRuntime,
    pub(super) initial_pending: Option<&'a Arc<StdMutex<Option<Vec<Message>>>>>,
    pub(super) process_handles: Arc<ProcessHandles>,
    pub(super) sqlite_sess:
        &'a mut Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
}

async fn repl_iteration_handle_shell_line(
    opt_cmd: Option<String>,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplMainIterationCtl {
    let wd = work_dir.to_path_buf();
    let sty = style.clone();
    match tokio::task::spawn_blocking(move || {
        repl_execute_shell(opt_cmd.as_deref(), wd.as_path(), &sty)
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = style.eprint_error(&e.to_string());
        }
        Err(e) => {
            let _ = style.eprint_error(&e.to_string());
        }
    }
    ReplMainIterationCtl::Continue
}

async fn repl_iteration_handle_chat_line(
    input: String,
    ctx: &mut ReplIterationCtx<'_>,
) -> Result<ReplMainIterationCtl, Box<dyn std::error::Error>> {
    if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
        return Ok(ReplMainIterationCtl::BreakRepl);
    }

    if repl_slash_branch_continue_loop(ReplSlashBranchContinueLoopParams {
        input: input.as_str(),
        cfg_holder: ctx.cfg_holder,
        config_path: ctx.config_path,
        tools: ctx.tools,
        messages: ctx.messages,
        work_dir: ctx.work_dir,
        style: ctx.style,
        no_stream: ctx.no_stream,
        agent_role_owned: ctx.agent_role_owned,
        slash_handles: ctx.slash_handles,
        client: ctx.client,
        sqlite_sess: ctx.sqlite_sess,
    })
    .await?
    {
        return Ok(ReplMainIterationCtl::Continue);
    }

    repl_dispatch_chat_round(ReplDispatchChatRoundParams {
        input,
        cfg_holder: ctx.cfg_holder,
        tools: ctx.tools,
        messages: ctx.messages,
        work_dir: ctx.work_dir,
        style: ctx.style,
        no_stream: ctx.no_stream,
        suppress_stdout_render: false,
        tui_llm_stream_scratch: None,
        tool_running_hook: None,
        after_user_message_enqueued: None,
        agent_role_owned: ctx.agent_role_owned,
        api_key_holder: ctx.api_key_holder,
        client: ctx.client,
        cli_rt: ctx.cli_rt,
        initial_pending: ctx.initial_pending,
        process_handles: Arc::clone(&ctx.process_handles),
        clarify_answers_for_next_user_message: None,
        clarification_questionnaire_hook: None,
        sse_control_mirror: None,
        session_mode: &ctx.slash_handles.session_mode,
    })
    .await?;
    if let Some(sess) = ctx.sqlite_sess.as_mut()
        && let Err(e) = sess.persist_round(ctx.messages, ctx.agent_role_owned.as_deref())
    {
        let _ = ctx
            .style
            .eprint_error(&format!("会话 SQLite 落盘失败: {e}"));
    }
    Ok(ReplMainIterationCtl::Continue)
}

pub(super) async fn repl_iteration_reply_to_read_line(
    read_res: ReplReadLine,
    ctx: &mut ReplIterationCtx<'_>,
) -> Result<ReplMainIterationCtl, Box<dyn std::error::Error>> {
    match read_res {
        ReplReadLine::Eof => Ok(ReplMainIterationCtl::BreakRepl),
        ReplReadLine::Empty => Ok(ReplMainIterationCtl::Continue),
        ReplReadLine::Shell(opt_cmd) => {
            Ok(repl_iteration_handle_shell_line(opt_cmd, ctx.work_dir.as_path(), ctx.style).await)
        }
        ReplReadLine::Chat(input) => repl_iteration_handle_chat_line(input, ctx).await,
    }
}
