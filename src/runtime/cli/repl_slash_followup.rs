//! REPL `/…` 命令在分派后的异步收尾（probe/models、mcp、热重载等）。

use crate::config::SharedAgentConfig;
use crate::runtime::cli::repl_extras::{ReplSlashHandled, ReplSlashSharedHandles};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui_terminal_bridge::{
    TuiTerminalHandoffOp, blocking_release_terminal, blocking_restore_terminal,
    pause_for_return_to_tui,
};

/// `/…` 命令在 [`try_handle_repl_slash_command`] 之后的异步收尾（probe/models、mcp、热重载等）。
pub(crate) struct ReplSlashFollowupCtx<'a> {
    pub cfg_holder: &'a SharedAgentConfig,
    pub config_path: Option<&'a str>,
    pub client: &'a reqwest::Client,
    pub slash_handles: &'a ReplSlashSharedHandles,
    pub style: &'a CliReplStyle,
    pub work_dir: &'a std::path::Path,
    /// **`crabmate tui`**：释放全屏后再执行写 stdout 的子逻辑。
    pub tui_terminal_tx: Option<&'a std::sync::mpsc::Sender<TuiTerminalHandoffOp>>,
}

async fn repl_slash_followup_with_optional_tui_handoff<F>(
    tui_terminal_tx: Option<&std::sync::mpsc::Sender<TuiTerminalHandoffOp>>,
    fut: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = ()> + Send,
{
    if let Some(tx) = tui_terminal_tx {
        let tx_c = tx.clone();
        tokio::task::spawn_blocking(move || blocking_release_terminal(&tx_c)).await??;
        fut.await;
        let tx_c = tx.clone();
        tokio::task::spawn_blocking(move || {
            let _ = pause_for_return_to_tui();
            blocking_restore_terminal(&tx_c)
        })
        .await??;
    } else {
        fut.await;
    }
    Ok(())
}

pub(crate) async fn repl_slash_handled_followup(
    handled: ReplSlashHandled,
    ctx: ReplSlashFollowupCtx<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    match handled {
        ReplSlashHandled::NotSlash | ReplSlashHandled::Handled => Ok(()),
        ReplSlashHandled::RunDoctor => {
            repl_slash_followup_with_optional_tui_handoff(ctx.tui_terminal_tx, async {
                let cfg = ctx.cfg_holder.read().await;
                let ws = ctx.work_dir.to_str();
                crate::runtime::cli_doctor::print_doctor_report(&cfg, ws);
            })
            .await
        }
        ReplSlashHandled::RunProbe => {
            repl_slash_followup_with_optional_tui_handoff(ctx.tui_terminal_tx, async {
                let g = ctx.cfg_holder.read().await;
                let k = ctx
                    .slash_handles
                    .api_key_holder
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Err(e) =
                    crate::runtime::cli_doctor::run_probe_cli(ctx.client, &g, k.trim()).await
                {
                    let _ = ctx.style.eprint_error(&e.to_string());
                }
            })
            .await
        }
        ReplSlashHandled::RunModels => {
            repl_slash_followup_with_optional_tui_handoff(ctx.tui_terminal_tx, async {
                let g = ctx.cfg_holder.read().await;
                let k = ctx
                    .slash_handles
                    .api_key_holder
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Err(e) =
                    crate::runtime::cli_doctor::run_models_cli(ctx.client, &g, k.trim()).await
                {
                    let _ = ctx.style.eprint_error(&e.to_string());
                }
            })
            .await
        }
        ReplSlashHandled::RunModelsChoose { model_id } => {
            repl_slash_followup_with_optional_tui_handoff(ctx.tui_terminal_tx, async {
                let (model_id, persist) = {
                    let t = model_id.trim();
                    if let Some(rest) = t.strip_suffix("--no-persist") {
                        (rest.trim_end().to_string(), false)
                    } else {
                        (t.to_string(), true)
                    }
                };
                let k = ctx
                    .slash_handles
                    .api_key_holder
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                match crate::runtime::cli_doctor::run_models_choose_repl(
                    ctx.client,
                    ctx.cfg_holder,
                    k.trim(),
                    &model_id,
                )
                .await
                {
                    Ok(resolved) => {
                        let mut note = format!("已设 model = {resolved}");
                        if persist {
                            let mut file = crate::user_data::load_llm_overrides();
                            file.client_llm.model = Some(resolved.clone());
                            match crate::user_data::save_llm_overrides(&file) {
                                Ok(()) => note.push_str("（已写入 user-data）"),
                                Err(e) => {
                                    let _ =
                                        ctx.style.eprint_error(&format!("写 user-data 失败: {e}"));
                                    note.push_str("（仅本进程内存）");
                                }
                            }
                        } else {
                            note.push_str("（仅本进程；--no-persist）");
                        }
                        let _ = ctx.style.print_success(&note);
                    }
                    Err(e) => {
                        let _ = ctx.style.eprint_error(&e.to_string());
                    }
                }
            })
            .await
        }
        ReplSlashHandled::RunMcpList { probe } => {
            repl_slash_followup_with_optional_tui_handoff(ctx.tui_terminal_tx, async {
                let g = ctx.cfg_holder.read().await;
                crate::runtime::cli_mcp::run_mcp_list(&g, probe, true).await;
            })
            .await
        }
        ReplSlashHandled::RunConfigReload => {
            match crate::runtime::config_reload::reload_shared_agent_config(
                ctx.cfg_holder,
                ctx.config_path,
            )
            .await
            {
                Ok(()) => {
                    let _ = ctx.style.print_success(
                        "配置已热重载（conversation_store_sqlite_path 与 HTTP Client 未重建；详见文档）。",
                    );
                }
                Err(e) => {
                    let _ = ctx.style.eprint_error(&e);
                }
            }
            Ok(())
        }
    }
}
