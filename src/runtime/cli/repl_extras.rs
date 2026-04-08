//! REPL `/…` 命令处理、首轮注入、内存导出。

use crate::config::cli::{SaveSessionCli, SaveSessionFormat};
use crate::config::{AgentConfig, LlmHttpAuthMode, SharedAgentConfig};
use crate::project_profile::build_first_turn_user_context_markdown;
use crate::runtime::cli::repl_parse::{
    ReplBuiltIn, classify_repl_slash_command, print_repl_version_line,
    repl_agent_role_set_is_default_pseudo,
};
use crate::runtime::cli::{ReplExportKind, run_save_session_command};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

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
    if messages.len() < 2 {
        return;
    }
    if !messages[0].role.trim().eq_ignore_ascii_case("system")
        || !messages[1].role.trim().eq_ignore_ascii_case("user")
    {
        return;
    }
    let cfg = cfg_holder.read().await.clone();
    let want_heavy = (cfg.project_profile_inject_enabled
        && cfg.project_profile_inject_max_chars > 0)
        || (cfg.project_dependency_brief_inject_enabled
            && cfg.project_dependency_brief_inject_max_chars > 0);
    let ctx: Option<String> = if want_heavy {
        let wd = work_dir.to_path_buf();
        let cfg_c = cfg.clone();
        tokio::task::spawn_blocking(move || {
            build_first_turn_user_context_markdown(&wd, &cfg_c, None)
        })
        .await
        .unwrap_or_default()
    } else {
        build_first_turn_user_context_markdown(work_dir, &cfg, None)
    };
    if let Some(body) = ctx {
        messages.insert(1, Message::user_only(body));
    }
}

/// 与启动时 [`crate::runtime::workspace_session::repl_bootstrap_messages_fast`] 同源：按当前 `agent_role` 重建首轮 `system`（及可选画像注入）。
pub(crate) async fn repl_rebuild_bootstrap_messages(
    cfg: &AgentConfig,
    work_dir: &Path,
    agent_role: Option<&str>,
) -> Vec<Message> {
    let system_prompt = match cfg.system_prompt_for_new_conversation(agent_role) {
        Ok(s) => s.to_string(),
        Err(_) => cfg.system_prompt.clone(),
    };
    let system_prompt = crate::tool_stats::augment_system_prompt(&system_prompt, cfg);
    let system_prompt_fb = system_prompt.clone();
    let wd = work_dir.to_path_buf();
    let cfg = cfg.clone();
    let want_heavy = (cfg.project_profile_inject_enabled
        && cfg.project_profile_inject_max_chars > 0)
        || (cfg.project_dependency_brief_inject_enabled
            && cfg.project_dependency_brief_inject_max_chars > 0);
    if want_heavy {
        match tokio::task::spawn_blocking(move || {
            if let Some(ctx) = build_first_turn_user_context_markdown(&wd, &cfg, None) {
                vec![
                    Message::system_only(system_prompt.clone()),
                    Message::user_only(ctx),
                ]
            } else {
                vec![Message::system_only(system_prompt)]
            }
        })
        .await
        {
            Ok(v) => v,
            Err(_) => vec![Message::system_only(system_prompt_fb)],
        }
    } else if let Some(ctx) = build_first_turn_user_context_markdown(work_dir, &cfg, None) {
        vec![Message::system_only(system_prompt), Message::user_only(ctx)]
    } else {
        vec![Message::system_only(system_prompt)]
    }
}

fn repl_export_kind_from_arg(arg: &str) -> Result<ReplExportKind, ()> {
    let a = arg.trim().to_ascii_lowercase();
    match a.as_str() {
        "" | "both" => Ok(ReplExportKind::Both),
        "json" => Ok(ReplExportKind::Json),
        "markdown" | "md" => Ok(ReplExportKind::Markdown),
        _ => Err(()),
    }
}

/// 将内存中的消息导出到工作区 `.crabmate/exports/`（与 Web 及 `save-session` 落盘形状同形）。
fn repl_export_current_messages(
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
    api_key_holder: &Arc<StdMutex<String>>,
) -> ReplSlashHandled {
    let Some(builtin) = classify_repl_slash_command(input) else {
        return ReplSlashHandled::NotSlash;
    };
    match builtin {
        ReplBuiltIn::BareSlash => {
            let _ = style.print_line(
                "输入 /help 查看内建命令；若以 / 开头的文字要发给模型，请避免仅输入一个 /。",
            );
        }
        ReplBuiltIn::Unknown(head) => {
            let _ = style.eprint_error(&format!("未知命令 /{head}。输入 /help 查看列表。"));
        }
        ReplBuiltIn::Clear => {
            let cfg = cfg_holder.read().await.clone();
            *messages =
                repl_rebuild_bootstrap_messages(&cfg, work_dir.as_path(), agent_role.as_deref())
                    .await;
            let _ = style.print_success(&format!(
                "已清空对话（保留当前 system 提示词），共 {} 条消息。",
                messages.len()
            ));
        }
        ReplBuiltIn::Model => {
            let cfg = cfg_holder.read().await;
            let _ = style.print_line(&format!("model: {}", cfg.model));
            let _ = style.print_line(&format!("api_base: {}", cfg.api_base));
            let _ = style.print_line(&format!(
                "temperature: {}（配置文件；Web chat 可单条覆盖）",
                cfg.temperature
            ));
            if let Some(seed) = cfg.llm_seed {
                let _ = style.print_line(&format!("llm_seed: {seed}"));
            } else {
                let _ = style.print_line("llm_seed: （未设置，请求不带 seed）");
            }
        }
        ReplBuiltIn::Config(extra) => {
            let e = extra.trim();
            if e.eq_ignore_ascii_case("reload") {
                return ReplSlashHandled::RunConfigReload;
            }
            if !e.is_empty() {
                let _ = style.eprint_error("用法: /config · /config reload（热重载，见文档）");
            } else {
                let cfg = cfg_holder.read().await;
                if let Err(err) = style.print_repl_config_summary(
                    &cfg,
                    work_dir.as_path(),
                    tools.len(),
                    no_stream,
                ) {
                    let _ = style.eprint_error(&err.to_string());
                }
            }
        }
        ReplBuiltIn::Doctor(extra) => {
            if !extra.is_empty() {
                let _ = style.eprint_error("用法: /doctor（无额外参数；同 crabmate doctor）");
            } else {
                let ws = work_dir.to_str();
                let cfg = cfg_holder.read().await;
                crate::runtime::cli_doctor::print_doctor_report(&cfg, ws);
            }
        }
        ReplBuiltIn::Probe(extra) => {
            if !extra.is_empty() {
                let _ = style.eprint_error("用法: /probe（无额外参数；同 crabmate probe）");
            } else {
                return ReplSlashHandled::RunProbe;
            }
        }
        ReplBuiltIn::ModelsList => {
            return ReplSlashHandled::RunModels;
        }
        ReplBuiltIn::ModelsChoose(model_id) => {
            return ReplSlashHandled::RunModelsChoose { model_id };
        }
        ReplBuiltIn::ModelsUsage => {
            let _ = style.eprint_error(
                "用法: /models · /models list（列模型）· /models choose <id>（从列表设当前 model；id 可唯一前缀）",
            );
        }
        ReplBuiltIn::WorkspaceShow => match work_dir.canonicalize() {
            Ok(p) => {
                let _ = style.print_line(&format!("当前工作区: {}", p.display()));
            }
            Err(_) => {
                let _ = style.print_line(&format!("当前工作区: {}", work_dir.display()));
            }
        },
        ReplBuiltIn::WorkspaceSet(arg) => {
            let cfg = cfg_holder.read().await;
            match crate::tools::resolve_repl_workspace_switch_path(&cfg, work_dir.as_path(), arg) {
                Ok(resolved) => {
                    *work_dir = resolved;
                    let _ = style.print_success(&format!("工作区已切换为: {}", work_dir.display()));
                }
                Err(e) => {
                    let _ = style.eprint_error(&e.to_string());
                }
            }
        }
        ReplBuiltIn::Tools => {
            if tools.is_empty() {
                let _ = style.print_line("当前未加载工具（可能使用了 --no-tools）。");
            } else {
                let _ = style.print_line(&format!("当前 {} 个工具:", tools.len()));
                for t in tools {
                    let _ = style.print_line(&format!("  · {}", t.function.name));
                }
            }
        }
        ReplBuiltIn::Help => {
            let _ = style.print_help();
        }
        ReplBuiltIn::Export(arg) => {
            let kind = match repl_export_kind_from_arg(arg) {
                Ok(k) => k,
                Err(()) => {
                    let _ = style.eprint_error("用法: /export 或 /export json | markdown | both");
                    return ReplSlashHandled::Handled;
                }
            };
            if let Err(e) = repl_export_current_messages(work_dir, messages, kind, style) {
                let _ = style.eprint_error(&e.to_string());
            }
        }
        ReplBuiltIn::SaveSession(arg) => {
            let kind = match repl_export_kind_from_arg(arg) {
                Ok(k) => k,
                Err(()) => {
                    let _ = style.eprint_error(
                        "用法: /save-session 或 /save-session json | markdown | both",
                    );
                    return ReplSlashHandled::Handled;
                }
            };
            let format = match kind {
                ReplExportKind::Json => SaveSessionFormat::Json,
                ReplExportKind::Markdown => SaveSessionFormat::Markdown,
                ReplExportKind::Both => SaveSessionFormat::Both,
            };
            let cli = SaveSessionCli {
                format,
                session_file: None,
            };
            let ws = Some(work_dir.to_string_lossy().into_owned());
            let cfg = cfg_holder.read().await;
            if let Err(e) = run_save_session_command(&cfg, &ws, cli) {
                let _ = style.eprint_error(&e.to_string());
            }
        }
        ReplBuiltIn::McpList { probe } => {
            return ReplSlashHandled::RunMcpList { probe };
        }
        ReplBuiltIn::McpUnknown(tail) => {
            let _ = style.eprint_error(&format!(
                "未知 /mcp 子命令: {tail}。用法: /mcp · /mcp list · /mcp probe · /mcp list probe"
            ));
        }
        ReplBuiltIn::AgentList => {
            let cfg = cfg_holder.read().await;
            if cfg.agent_roles.is_empty() {
                let _ = style.print_line(
                    "当前配置未启用多角色（agent_roles 为空）。可在配置中加入 [[agent_roles]] 或 config/agent_roles.toml。",
                );
            } else {
                let mut ids: Vec<&String> = cfg.agent_roles.keys().collect();
                ids.sort();
                let def = cfg.default_agent_role_id.as_deref();
                let _ = style.print_line("可用角色 id：");
                let _ = style.print_line(
                    "  · default（内建：未显式选用命名角色；与 Web「默认」一致：先按 default_agent_role_id，未配置则用全局 system_prompt）",
                );
                for id in ids {
                    let mark = def.is_some_and(|d| d == id.as_str());
                    let suffix = if mark { "（配置默认）" } else { "" };
                    let _ = style.print_line(&format!("  · {id}{suffix}"));
                }
                let cur = agent_role.as_deref().filter(|s| !s.is_empty()).map_or_else(
                    || "当前 REPL: default（未显式设置命名角色）".to_string(),
                    |r| format!("当前 REPL 选用命名角色: {r}"),
                );
                let _ = style.print_line(&cur);
            }
        }
        ReplBuiltIn::AgentSet(id) => {
            let cfg = cfg_holder.read().await;
            if cfg.agent_roles.is_empty() {
                let _ = style.eprint_error(
                    "当前未配置多角色，无法 /agent set。请先配置 [[agent_roles]] 或 agent_roles.toml。",
                );
            } else if repl_agent_role_set_is_default_pseudo(id.as_str()) {
                drop(cfg);
                *agent_role = None;
                let cfg = cfg_holder.read().await.clone();
                *messages = repl_rebuild_bootstrap_messages(
                    &cfg,
                    work_dir.as_path(),
                    agent_role.as_deref(),
                )
                .await;
                let _ = style.print_success(&format!(
                    "已设回 default（清除显式命名角色），并已按新 system 重建首轮消息（共 {} 条）。",
                    messages.len()
                ));
            } else if let Err(e) = cfg.system_prompt_for_new_conversation(Some(id.as_str())) {
                let _ = style.eprint_error(&e);
            } else {
                let role_label = id.clone();
                drop(cfg);
                *agent_role = Some(id);
                let cfg = cfg_holder.read().await.clone();
                *messages = repl_rebuild_bootstrap_messages(
                    &cfg,
                    work_dir.as_path(),
                    agent_role.as_deref(),
                )
                .await;
                let _ = style.print_success(&format!(
                    "已设当前角色为 \"{role_label}\"，并已按新 system 重建首轮消息（共 {} 条）。",
                    messages.len()
                ));
            }
        }
        ReplBuiltIn::AgentUsage => {
            let _ = style.eprint_error(
                "用法: /agent · /agent list（列角色 id，含内建 default）· /agent set <id> | /agent set default（default=清除显式角色，回到与 Web 默认相同逻辑）",
            );
        }
        ReplBuiltIn::Version => {
            print_repl_version_line();
        }
        ReplBuiltIn::ApiKeyUsage => {
            let _ = style.print_line(
                "用法: /api-key status（是否已在本进程设置密钥）· /api-key set <密钥> · /api-key clear",
            );
            let _ = style.print_line(
                "说明: 密钥仅存本进程内存，不写盘；未设置环境变量 API_KEY 时可用此命令。/config reload 不会清除此处设置的值。",
            );
        }
        ReplBuiltIn::ApiKeyStatus => {
            let g = cfg_holder.read().await;
            let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
            let set = !k.trim().is_empty();
            drop(k);
            if g.llm_http_auth_mode == LlmHttpAuthMode::None {
                let _ = style.print_line(
                    "当前 llm_http_auth_mode=none：发往 LLM 的请求不附带 Bearer，通常无需配置 API 密钥。",
                );
            } else if set {
                let _ = style.print_success("本进程已设置 LLM API 密钥（非空，值已隐藏）。");
            } else {
                let _ = style.print_line(
                    "本进程尚未设置 LLM API 密钥（环境变量 API_KEY 与 /api-key 均为空）；发消息前请 /api-key set <密钥> 或 export API_KEY 后重启。",
                );
            }
        }
        ReplBuiltIn::ApiKeyClear => {
            api_key_holder
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            let _ = style
                .print_success("已清除本进程内存中的 LLM API 密钥（环境变量 API_KEY 不受影响）。");
        }
        ReplBuiltIn::ApiKeySet(secret) => {
            if secret.len() > 16384 {
                let _ = style.eprint_error("密钥过长（上限 16384 字符）。");
            } else {
                *api_key_holder.lock().unwrap_or_else(|e| e.into_inner()) = secret;
                let _ = style.print_success("已写入本进程 LLM API 密钥（仅存内存；值已隐藏）。");
            }
        }
    }
    ReplSlashHandled::Handled
}
