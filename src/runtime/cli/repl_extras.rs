//! REPL `/…` 命令处理、首轮注入、内存导出。

use crate::config::AgentConfig;
use crate::config::SharedAgentConfig;
use crate::context_bootstrap::conversation_turn_bootstrap::{
    augmented_system_for_new_conversation_lenient, compose_new_conversation_messages,
    first_turn_project_context_user_message_sync,
};
use crate::process_handles::ProcessHandles;
use crate::runtime::cli::ReplExportKind;
use crate::runtime::cli::repl_parse::classify_repl_slash_command;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

/// 与 Web **`client_llm`** 校验上限对齐（仅本进程内存覆盖）。
pub(super) const REPL_LLM_API_BASE_MAX: usize = 2048;
pub(super) const REPL_LLM_MODEL_MAX: usize = 512;

/// REPL `/…` 命令：`api_key_holder` 与 [`ProcessHandles`] 合并传递以降低顶层函数形参计数。
pub(crate) struct ReplSlashSharedHandles {
    pub api_key_holder: Arc<StdMutex<String>>,
    pub process_handles: Arc<ProcessHandles>,
}

/// [`try_handle_repl_slash_command`] 的返回值：`RunProbe` / `RunModels` / `RunModelsChoose` 需在异步上下文中分别调用
/// [`crate::runtime::cli_doctor::run_probe_cli`]、[`crate::runtime::cli_doctor::run_models_cli`]、
/// [`crate::runtime::cli_doctor::run_models_choose_repl`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplSlashHandled {
    NotSlash,
    Handled,
    RunProbe,
    RunModels,
    RunModelsChoose {
        model_id: String,
    },
    /// 同 `crabmate mcp list`（`probe` 会启动 MCP 子进程）
    RunMcpList {
        probe: bool,
    },
    /// `/config reload`：磁盘+环境变量热更（见 `apply_hot_reload_config_subset`）
    RunConfigReload,
}

/// `chat` / REPL 首轮在 `[system, user]` 之间插入项目画像 + 依赖摘要（与 Web 同源）；`--messages-json-file` 等已带完整 transcript 时不调用。
pub(crate) async fn prepend_cli_first_turn_injection(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    messages: &mut Vec<Message>,
) {
    crate::context_bootstrap::conversation_turn_bootstrap::prepend_first_turn_project_context_between_system_and_user(
        cfg_holder, work_dir, messages,
    )
    .await;
}

/// 与启动时 [`crate::runtime::workspace_session::repl_bootstrap_messages_fast`] 同源：按当前 `agent_role` 重建首轮 `system`（及可选画像注入）。
pub(crate) async fn repl_rebuild_bootstrap_messages(
    cfg: &AgentConfig,
    work_dir: &Path,
    agent_role: Option<&str>,
    tool_recorder: &std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
) -> Vec<Message> {
    let system_prompt =
        augmented_system_for_new_conversation_lenient(cfg, agent_role, tool_recorder);
    let system_prompt_fb = system_prompt.clone();
    let cfg = cfg.clone();
    let wd = work_dir.to_path_buf();
    if crate::context_bootstrap::conversation_turn_bootstrap::project_scan_needs_spawn_blocking(
        &cfg,
    ) {
        match tokio::task::spawn_blocking(move || {
            let ctx = first_turn_project_context_user_message_sync(wd.as_path(), &cfg, None);
            compose_new_conversation_messages(&system_prompt, ctx, None)
        })
        .await
        {
            Ok(v) => v,
            Err(_) => vec![Message::system_only(system_prompt_fb)],
        }
    } else {
        let ctx = first_turn_project_context_user_message_sync(work_dir, &cfg, None);
        compose_new_conversation_messages(&system_prompt, ctx, None)
    }
}

pub(super) fn repl_export_kind_from_arg(arg: &str) -> Result<ReplExportKind, ()> {
    let a = arg.trim().to_ascii_lowercase();
    match a.as_str() {
        "" | "both" => Ok(ReplExportKind::Both),
        "json" => Ok(ReplExportKind::Json),
        "markdown" | "md" => Ok(ReplExportKind::Markdown),
        _ => Err(()),
    }
}

/// 将内存中的消息导出到工作区 `.crabmate/exports/`（与 Web 及 `save-session` 落盘形状同形）。
pub(super) fn repl_export_current_messages(
    work_dir: &Path,
    messages: &[Message],
    kind: ReplExportKind,
    style: &CliReplStyle,
) -> io::Result<()> {
    match kind {
        ReplExportKind::Json => {
            let p = crate::runtime::workspace_session::export_json(work_dir, messages)?;
            style.print_success(&format!("已导出 JSON: {}", p.display()))?;
        }
        ReplExportKind::Markdown => {
            let p = crate::runtime::workspace_session::export_markdown(work_dir, messages)?;
            style.print_success(&format!("已导出 Markdown: {}", p.display()))?;
        }
        ReplExportKind::Both => {
            let pj = crate::runtime::workspace_session::export_json(work_dir, messages)?;
            let pm = crate::runtime::workspace_session::export_markdown(work_dir, messages)?;
            style.print_success(&format!("已导出 JSON: {}", pj.display()))?;
            style.print_success(&format!("已导出 Markdown: {}", pm.display()))?;
        }
    }
    Ok(())
}

/// REPL 中以 `/` 开头的内建命令；[`ReplSlashHandled::NotSlash`] 时应将输入交给模型。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn try_handle_repl_slash_command(
    input: &str,
    cfg_holder: &SharedAgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &mut std::path::PathBuf,
    style: &CliReplStyle,
    no_stream: bool,
    agent_role: &mut Option<String>,
    handles: &ReplSlashSharedHandles,
) -> ReplSlashHandled {
    let Some(builtin) = classify_repl_slash_command(input) else {
        return ReplSlashHandled::NotSlash;
    };
    super::repl_slash_dispatch::dispatch_repl_slash_builtin(
        builtin, cfg_holder, tools, messages, work_dir, style, no_stream, agent_role, handles,
    )
    .await
}
