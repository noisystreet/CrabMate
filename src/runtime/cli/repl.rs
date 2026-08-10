//! 交互式 REPL 主循环。
//!
//! **CLI 入口已移除**（D2.1）；本模块暂留至 D2.2 删除。
#![allow(dead_code)]

pub(crate) use super::repl_bootstrap::repl_prepare_messages_and_editor;
pub(crate) use super::repl_chat_round::{
    ReplAfterUserMessageEnqueuedCb, ReplDispatchChatRoundParams, repl_dispatch_chat_round,
};
pub(crate) use super::repl_slash_followup::{ReplSlashFollowupCtx, repl_slash_handled_followup};

use super::repl_iteration::{
    ReplIterationCtx, ReplMainIterationCtl, repl_iteration_reply_to_read_line,
};
use crate::config::SharedAgentConfig;
use crate::runtime::cli::chat::CliMainInvocationCommon;
use crate::runtime::cli::cli_effective_work_dir;
use crate::runtime::cli::repl_extras::ReplSlashSharedHandles;
use crate::runtime::cli_exit::{CliExitError, EXIT_USAGE};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::repl_reedline::{ReplLineEditor, read_repl_line_with_editor};
use crate::tool_registry::CliToolRuntime;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

async fn repl_validate_role_and_print_banner(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    tools_len: usize,
    no_stream: bool,
    api_key: &str,
    agent_role: Option<&str>,
    style: &CliReplStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    let g = cfg_holder.read().await;
    if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
        g.system_prompt_for_new_conversation(Some(r))
            .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
    }
    let repl_llm_bearer_key_ready = !api_key.trim().is_empty();
    style.print_banner(
        &g,
        work_dir.as_ref(),
        tools_len,
        no_stream,
        repl_llm_bearer_key_ready,
    )?;
    Ok(())
}

async fn repl_refresh_skill_slash_completions(cfg_holder: &SharedAgentConfig, work_dir: &Path) {
    let g = cfg_holder.read().await;
    let items = if g.skills.skills_enabled {
        crate::config::skills_slash::list_skill_catalog_entries(g.skills.list_opts(work_dir))
            .unwrap_or_default()
            .into_iter()
            .map(
                |e| crate::runtime::repl_slash_complete::SkillSlashCompleteItem {
                    id: e.id,
                    description: e.description,
                },
            )
            .collect()
    } else {
        Vec::new()
    };
    crate::runtime::repl_slash_complete::refresh_skill_slash_completions(items);
}

async fn repl_read_line_from_editor(
    repl_editor: Arc<StdMutex<ReplLineEditor>>,
) -> Result<crate::runtime::repl_reedline::ReplReadLine, Box<dyn std::error::Error>> {
    tokio::task::spawn_blocking(move || {
        let mut guard = repl_editor.lock().unwrap_or_else(|e| e.into_inner());
        read_repl_line_with_editor(&mut guard)
    })
    .await
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
}

fn repl_print_sqlite_enabled_notice(
    style: &CliReplStyle,
    sqlite_sess: &Option<crate::runtime::cli_sqlite_session::CliSqliteSessionState>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(s) = sqlite_sess {
        let _ = style.print_line(&format!(
            "会话 SQLite 已启用 · conversation_id={}（/conv · /branch；CM_CONVERSATION_ID 可指定启动 id）",
            s.conversation_id
        ));
    }
    Ok(())
}

/// 交互式 REPL 模式
pub async fn run_repl(
    common: CliMainInvocationCommon<'_>,
    no_stream: bool,
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
    let (run_root, tui_load) = {
        let g = cfg_holder.read().await;
        (
            g.command_exec.run_command_working_dir.clone(),
            g.session_ui.tui_load_session_on_start,
        )
    };
    let mut work_dir = cli_effective_work_dir(workspace_cli, &run_root);
    let cli_rt = CliToolRuntime::new_interactive_default();
    let style = CliReplStyle::new();
    let api_key_holder = Arc::new(StdMutex::new(api_key.to_string()));
    let default_session_mode = {
        let g = cfg_holder.read().await;
        crate::session_mode_turn::resolve_initial_session_mode(&g, agent_role)
    };
    let slash_handles = ReplSlashSharedHandles {
        api_key_holder: Arc::clone(&api_key_holder),
        process_handles: Arc::clone(&process_handles),
        session_mode: Arc::new(StdMutex::new(default_session_mode)),
    };

    repl_validate_role_and_print_banner(
        cfg_holder,
        work_dir.as_path(),
        tools.len(),
        no_stream,
        api_key,
        agent_role,
        &style,
    )
    .await?;

    let agent_role_owned = agent_role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // `repl_initial_workspace_messages_enabled` 为 true 时：`initial_workspace_messages` 在独立线程中构建，不阻塞 REPL。
    let (messages, initial_pending, repl_editor) = repl_prepare_messages_and_editor(
        cfg_holder,
        tui_load,
        &work_dir,
        &agent_role_owned,
        run_root.as_str(),
        Arc::clone(&process_handles),
    )
    .await?;

    let (mut sqlite_sess, messages_sq, role_sq) =
        crate::runtime::cli_sqlite_session::maybe_bootstrap_cli_sqlite(
            cfg_holder,
            messages,
            agent_role_owned,
        )
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let mut messages = messages_sq;
    let mut agent_role_owned = role_sq;
    // SQLite 多会话启用时勿再并入 tui_session.json 后台恢复，避免双轨覆盖。
    let initial_pending = if sqlite_sess.is_some() {
        None
    } else {
        initial_pending
    };
    repl_print_sqlite_enabled_notice(&style, &sqlite_sess)?;

    loop {
        crate::runtime::workspace_session::try_merge_background_initial_workspace(
            &mut messages,
            initial_pending.as_ref(),
        );

        repl_refresh_skill_slash_completions(cfg_holder, work_dir.as_path()).await;

        let read_res = repl_read_line_from_editor(repl_editor.clone()).await?;

        let mut iter_ctx = ReplIterationCtx {
            cfg_holder,
            config_path,
            client,
            tools,
            messages: &mut messages,
            work_dir: &mut work_dir,
            style: &style,
            no_stream,
            agent_role_owned: &mut agent_role_owned,
            slash_handles: &slash_handles,
            api_key_holder: &api_key_holder,
            cli_rt: &cli_rt,
            initial_pending: initial_pending.as_ref(),
            process_handles: Arc::clone(&process_handles),
            sqlite_sess: &mut sqlite_sess,
        };
        match repl_iteration_reply_to_read_line(read_res, &mut iter_ctx).await? {
            ReplMainIterationCtl::BreakRepl => break,
            ReplMainIterationCtl::Continue => {}
        }
    }

    style.print_farewell()?;
    Ok(())
}
