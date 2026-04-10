//! 交互式 REPL 主循环。

use crate::config::{LlmHttpAuthMode, SharedAgentConfig};
use crate::redact;
use crate::runtime::cli::chat::run_agent_turn_for_cli;
use crate::runtime::cli::cli_effective_work_dir;
use crate::runtime::cli::repl_extras::{ReplSlashHandled, try_handle_repl_slash_command};
use crate::runtime::cli::repl_parse::run_repl_shell_line_sync;
use crate::runtime::cli_exit::CliExitError;
use crate::runtime::cli_exit::EXIT_USAGE;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::repl_reedline::{ReplLineEditor, ReplReadLine, read_repl_line_with_editor};
use crate::tool_registry::CliToolRuntime;
use crate::types::Message;
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use log::debug;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

const REPL_SHELL_USAGE: &str = "bash#: <命令>  在当前工作区执行一行 shell（不发给模型；无交互 stdin）。等同本机 `sh -c` / `cmd /C`，不受模型 `run_command` 白名单约束，仅应在可信环境使用。交互 TTY：空行按 `$` 即切换「我:」/ bash#:（也可单独一行 `$` 后 Enter）；管道/非 TTY 仍可用行内 `$ <命令>`。历史保存在工作区 `.crabmate/repl_history.txt`。示例: ls  pwd  git status";

/// 执行 REPL 本地 shell 一行：`parsed` 为 `repl_reedline::parse_repl_dollar_shell_line` 的 `Some(...)` 内层；`None` 表示仅 `$` 或空命令，打印用法。
fn repl_execute_shell(
    parsed: Option<&str>,
    work_dir: &std::path::Path,
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

/// 交互式 REPL 模式
#[allow(clippy::too_many_arguments)]
pub async fn run_repl(
    cfg_holder: &SharedAgentConfig,
    config_path: Option<&str>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    no_stream: bool,
    agent_role: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (run_root, tui_load) = {
        let g = cfg_holder.read().await;
        (
            g.run_command_working_dir.clone(),
            g.tui_load_session_on_start,
        )
    };
    let mut work_dir = cli_effective_work_dir(workspace_cli, &run_root);
    let cli_rt = CliToolRuntime::new_interactive_default();
    let style = CliReplStyle::new();
    let api_key_holder = Arc::new(StdMutex::new(api_key.to_string()));

    {
        let g = cfg_holder.read().await;
        if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            g.system_prompt_for_new_conversation(Some(r))
                .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
        }
        let repl_llm_bearer_key_ready = !api_key.trim().is_empty();
        style.print_banner(
            &g,
            work_dir.as_path(),
            tools.len(),
            no_stream,
            repl_llm_bearer_key_ready,
        )?;
    }

    let mut agent_role_owned = agent_role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // `repl_initial_workspace_messages_enabled` 为 true 时：`initial_workspace_messages` 在独立线程中构建，不阻塞 REPL。
    let (mut messages, initial_pending) = {
        let g = cfg_holder.read().await;
        let fast = crate::runtime::workspace_session::repl_bootstrap_messages_fast(
            &g,
            agent_role_owned.as_deref(),
        );
        if !g.repl_initial_workspace_messages_enabled {
            (fast, None)
        } else {
            let may_scan_workspace = (g.project_profile_inject_enabled
                && g.project_profile_inject_max_chars > 0)
                || (g.project_dependency_brief_inject_enabled
                    && g.project_dependency_brief_inject_max_chars > 0)
                || (g.agent_memory_file_enabled && !g.agent_memory_file.trim().is_empty());
            if may_scan_workspace || tui_load {
                let _ = writeln!(
                    io::stderr(),
                    "（后台正在准备工作区首轮上下文或会话恢复，可立即输入；就绪后将并入对话。）"
                );
                let _ = io::stderr().flush();
            }
            let cfg_bg = g.clone();
            let slot: Arc<StdMutex<Option<Vec<crate::types::Message>>>> =
                Arc::new(StdMutex::new(None));
            let slot_bg = Arc::clone(&slot);
            let wd_bg = work_dir.clone();
            let role_for_bg = agent_role_owned.clone();
            std::thread::spawn(move || {
                let built = crate::runtime::workspace_session::initial_workspace_messages(
                    &cfg_bg,
                    wd_bg.as_path(),
                    tui_load,
                    role_for_bg.as_deref(),
                );
                let mut guard = slot_bg.lock().unwrap_or_else(|e| e.into_inner());
                *guard = Some(built);
            });
            (fast, Some(slot))
        }
    };

    let history_dir = PathBuf::from(&run_root).join(".crabmate");
    std::fs::create_dir_all(&history_dir)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let history_file = history_dir.join("repl_history.txt");
    let repl_editor = Arc::new(StdMutex::new(
        ReplLineEditor::new(history_file.as_path())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
    ));

    loop {
        crate::runtime::workspace_session::try_merge_background_initial_workspace(
            &mut messages,
            initial_pending.as_ref(),
        );

        let ed = repl_editor.clone();
        let read_res = tokio::task::spawn_blocking(move || {
            let mut guard = ed.lock().unwrap_or_else(|e| e.into_inner());
            read_repl_line_with_editor(&mut guard)
        })
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        match read_res {
            ReplReadLine::Eof => break,
            ReplReadLine::Empty => continue,
            ReplReadLine::Shell(opt_cmd) => {
                let wd = work_dir.clone();
                let sty = style;
                match tokio::task::spawn_blocking(move || {
                    repl_execute_shell(opt_cmd.as_deref(), wd.as_path(), &sty)
                })
                .await
                {
                    Ok(Ok(())) => continue,
                    Ok(Err(e)) => {
                        let _ = style.eprint_error(&e.to_string());
                        continue;
                    }
                    Err(e) => {
                        let _ = style.eprint_error(&e.to_string());
                        continue;
                    }
                }
            }
            ReplReadLine::Chat(input) => {
                if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
                    break;
                }

                match try_handle_repl_slash_command(
                    input.as_str(),
                    cfg_holder,
                    tools,
                    &mut messages,
                    &mut work_dir,
                    &style,
                    no_stream,
                    &mut agent_role_owned,
                    &api_key_holder,
                )
                .await
                {
                    ReplSlashHandled::NotSlash => {}
                    ReplSlashHandled::Handled => continue,
                    ReplSlashHandled::RunProbe => {
                        let g = cfg_holder.read().await;
                        let k = api_key_holder
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        if let Err(e) =
                            crate::runtime::cli_doctor::run_probe_cli(client, &g, k.trim()).await
                        {
                            let _ = style.eprint_error(&e.to_string());
                        }
                        continue;
                    }
                    ReplSlashHandled::RunModels => {
                        let g = cfg_holder.read().await;
                        let k = api_key_holder
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        if let Err(e) =
                            crate::runtime::cli_doctor::run_models_cli(client, &g, k.trim()).await
                        {
                            let _ = style.eprint_error(&e.to_string());
                        }
                        continue;
                    }
                    ReplSlashHandled::RunModelsChoose { model_id } => {
                        let k = api_key_holder
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        match crate::runtime::cli_doctor::run_models_choose_repl(
                            client,
                            cfg_holder,
                            k.trim(),
                            &model_id,
                        )
                        .await
                        {
                            Ok(resolved) => {
                                let _ = style.print_success(&format!(
                                    "已设 model = {resolved}（仅本进程有效；持久化请改配置文件；/config reload 会从磁盘覆盖）"
                                ));
                            }
                            Err(e) => {
                                let _ = style.eprint_error(&e.to_string());
                            }
                        }
                        continue;
                    }
                    ReplSlashHandled::RunMcpList { probe } => {
                        let g = cfg_holder.read().await;
                        crate::runtime::cli_mcp::run_mcp_list(&g, probe, true).await;
                        continue;
                    }
                    ReplSlashHandled::RunConfigReload => {
                        match crate::runtime::config_reload::reload_shared_agent_config(
                            cfg_holder,
                            config_path,
                        )
                        .await
                        {
                            Ok(()) => {
                                let _ = style.print_success(
                                    "配置已热重载（conversation_store_sqlite_path 与 HTTP Client 未重建；详见文档）。",
                                );
                            }
                            Err(e) => {
                                let _ = style.eprint_error(&e);
                            }
                        }
                        continue;
                    }
                }

                crate::runtime::workspace_session::try_merge_background_initial_workspace(
                    &mut messages,
                    initial_pending.as_ref(),
                );
                {
                    let g = cfg_holder.read().await;
                    if g.llm_http_auth_mode == LlmHttpAuthMode::Bearer {
                        let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
                        if k.trim().is_empty() {
                            drop(k);
                            let _ = style.eprint_error(
                                "当前为 llm_http_auth_mode=bearer，但未配置 LLM API 密钥。请执行 /api-key set <密钥>（仅本进程）或设置环境变量 API_KEY 后重启。",
                            );
                            continue;
                        }
                    }
                }
                let user_body = {
                    let g = cfg_holder.read().await;
                    match expand_at_file_refs_in_user_message(
                        input.as_str(),
                        work_dir.as_path(),
                        &g,
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = style.eprint_error(&e);
                            continue;
                        }
                    }
                };
                messages.push(Message::user_only(user_body));
                debug!(
                    target: "crabmate::print",
                    "REPL 用户输入已入队 history_len={} input_preview={}",
                    messages.len(),
                    redact::preview_chars(input.as_str(), redact::MESSAGE_LOG_PREVIEW_CHARS)
                );

                let cfg_snap = {
                    let g = cfg_holder.read().await;
                    Arc::new(g.clone())
                };
                let key_snap = api_key_holder
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Err(e) = run_agent_turn_for_cli(
                    client,
                    key_snap.as_str(),
                    &cfg_snap,
                    tools,
                    &mut messages,
                    work_dir.as_path(),
                    no_stream,
                    Some(&cli_rt),
                )
                .await
                {
                    let _ = style.eprint_error(&format!(
                        "本轮对话失败（可继续输入；异常历史可 /clear 清空）：{}",
                        e
                    ));
                    continue;
                }
            }
        }
    }

    style.print_farewell()?;
    Ok(())
}
