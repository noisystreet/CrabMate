//! REPL `/…` 命令分派：从 [`super::repl_extras::try_handle_repl_slash_command`] 拆出以降低单函数圈复杂度。

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::agent_role_turn::apply_agent_role_switch_to_messages;
use crate::config::LlmHttpAuthMode;
use crate::config::SharedAgentConfig;
use crate::config::cli::{SaveSessionCli, SaveSessionFormat};
use crate::llm::vendor::refresh_llm_reasoning_split_for_gateway;
use crate::runtime::cli::repl_parse::{
    ReplBuiltIn, print_repl_version_line, repl_agent_role_set_is_default_pseudo,
};
use crate::runtime::cli::{ReplExportKind, run_save_session_command};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;

use super::repl_extras::{
    REPL_LLM_API_BASE_MAX, REPL_LLM_MODEL_MAX, ReplSlashHandled, ReplSlashSharedHandles,
    repl_export_current_messages, repl_export_kind_from_arg, repl_rebuild_bootstrap_messages,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_repl_slash_builtin<'a>(
    builtin: ReplBuiltIn<'a>,
    cfg_holder: &SharedAgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &mut std::path::PathBuf,
    style: &CliReplStyle,
    no_stream: bool,
    agent_role: &mut Option<String>,
    handles: &ReplSlashSharedHandles,
) -> ReplSlashHandled {
    match builtin {
        ReplBuiltIn::BareSlash => {
            let _ = style.print_line(
                "输入 /help 查看内建命令；若以 / 开头的文字要发给模型，请避免仅输入一个 /。",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Unknown(head) => {
            let _ = style.eprint_error(&format!("未知命令 /{head}。输入 /help 查看列表。"));
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Clear => {
            slash_clear(
                cfg_holder,
                messages,
                work_dir.as_path(),
                agent_role,
                style,
                &handles.process_handles.tool_outcome_recorder,
            )
            .await
        }
        ReplBuiltIn::ModelShow => slash_model_show(cfg_holder, style).await,
        ReplBuiltIn::ModelSet(name) => slash_model_set(name, cfg_holder, style).await,
        ReplBuiltIn::ModelUsage => slash_model_usage(style),
        ReplBuiltIn::ApiBaseShow => slash_api_base_show(cfg_holder, style).await,
        ReplBuiltIn::ApiBaseSet(url) => slash_api_base_set(url, cfg_holder, style).await,
        ReplBuiltIn::ApiBaseUsage => slash_api_base_usage(style),
        ReplBuiltIn::Config(extra) => {
            slash_config(
                extra,
                cfg_holder,
                work_dir.as_path(),
                tools,
                style,
                no_stream,
            )
            .await
        }
        ReplBuiltIn::Doctor(extra) => {
            slash_doctor(extra, cfg_holder, work_dir.as_path(), style).await
        }
        ReplBuiltIn::Probe(extra) => slash_probe(extra, style),
        ReplBuiltIn::ModelsList => ReplSlashHandled::RunModels,
        ReplBuiltIn::ModelsChoose(model_id) => ReplSlashHandled::RunModelsChoose { model_id },
        ReplBuiltIn::ModelsUsage => {
            let _ = style.eprint_error(
                "用法: /models · /models list（列模型）· /models choose <id>（从列表设当前 model；id 可唯一前缀）",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::WorkspaceShow => slash_workspace_show(work_dir.as_path(), style),
        ReplBuiltIn::WorkspaceSet(arg) => {
            slash_workspace_set(cfg_holder, work_dir, arg, style).await
        }
        ReplBuiltIn::SkillsList => slash_skills_list(cfg_holder, work_dir.as_path(), style).await,
        ReplBuiltIn::Tools => slash_tools_list(tools, style),
        ReplBuiltIn::Help => {
            let _ = style.print_help();
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Export(arg) => slash_export(arg, work_dir.as_path(), messages, style),
        ReplBuiltIn::SaveSession(arg) => {
            slash_save_session(arg, cfg_holder, work_dir.as_path(), style).await
        }
        ReplBuiltIn::McpList { probe } => ReplSlashHandled::RunMcpList { probe },
        ReplBuiltIn::McpUnknown(tail) => {
            let _ = style.eprint_error(&format!(
                "未知 /mcp 子命令: {tail}。用法: /mcp · /mcp list · /mcp probe · /mcp list probe"
            ));
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::AgentList => slash_agent_list(cfg_holder, agent_role, style).await,
        ReplBuiltIn::AgentSet(id) => {
            slash_agent_set(
                id,
                cfg_holder,
                messages,
                agent_role,
                style,
                &handles.process_handles.tool_outcome_recorder,
            )
            .await
        }
        ReplBuiltIn::AgentUsage => {
            let _ = style.eprint_error(
                "用法: /agent · /agent list（列角色 id，含内建 default）· /agent set <id> | /agent set default（default=清除显式角色，回到与 Web 默认相同逻辑）",
            );
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::Version => {
            print_repl_version_line();
            ReplSlashHandled::Handled
        }
        ReplBuiltIn::ApiKeyUsage => slash_api_key_usage(style),
        ReplBuiltIn::ApiKeyStatus => {
            slash_api_key_status(cfg_holder, &handles.api_key_holder, style).await
        }
        ReplBuiltIn::ApiKeyClear => slash_api_key_clear(&handles.api_key_holder, style),
        ReplBuiltIn::ApiKeySet(secret) => slash_api_key_set(secret, &handles.api_key_holder, style),
    }
}

async fn slash_clear(
    cfg_holder: &SharedAgentConfig,
    messages: &mut Vec<Message>,
    work_dir: &Path,
    agent_role: &mut Option<String>,
    style: &CliReplStyle,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await.clone();
    *messages =
        repl_rebuild_bootstrap_messages(&cfg, work_dir, agent_role.as_deref(), tool_recorder).await;
    let _ = style.print_success(&format!(
        "已清空对话（保留当前 system 提示词），共 {} 条消息。",
        messages.len()
    ));
    ReplSlashHandled::Handled
}

async fn slash_model_show(
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    let _ = style.print_line(&format!("model: {}", cfg.llm.model));
    let _ = style.print_line(&format!("api_base: {}", cfg.llm.api_base));
    let _ = style.print_line(&format!(
        "temperature: {}（配置文件；Web chat 可单条覆盖）",
        cfg.llm_sampling.temperature
    ));
    if let Some(seed) = cfg.llm_sampling.llm_seed {
        let _ = style.print_line(&format!("llm_seed: {seed}"));
    } else {
        let _ = style.print_line("llm_seed: （未设置，请求不带 seed）");
    }
    let _ = style.print_line(
        "提示: /model set <名称> 可直接改模型 id；/api-base set <url> 可改网关根地址（均仅内存，/config reload 会从磁盘覆盖）。",
    );
    ReplSlashHandled::Handled
}

async fn slash_model_set(
    name: String,
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let t = name.trim();
    if t.is_empty() {
        let _ = style.eprint_error(
            "用法: /model set <模型名或 id>（可与 /models list 列出的 id 不同，不校验列表）",
        );
    } else if t.len() > REPL_LLM_MODEL_MAX {
        let _ = style.eprint_error(&format!("model 过长（上限 {REPL_LLM_MODEL_MAX} 字符）。"));
    } else {
        let label = t.to_string();
        let mut w = cfg_holder.write().await;
        w.llm.model.clone_from(&label);
        refresh_llm_reasoning_split_for_gateway(&mut w);
        drop(w);
        let _ = style.print_success(&format!(
            "已设 model = {label}（仅本进程；持久化请改配置；/config reload 会从磁盘覆盖；llm_reasoning_split 已按网关默认刷新）"
        ));
    }
    ReplSlashHandled::Handled
}

fn slash_model_usage(style: &CliReplStyle) -> ReplSlashHandled {
    let _ = style.eprint_error(
        "用法: /model（显示当前）· /model set <模型名或 id>（写内存；不校验 GET /models）",
    );
    ReplSlashHandled::Handled
}

async fn slash_api_base_show(
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    let _ = style.print_line(&format!("api_base: {}", cfg.llm.api_base));
    let _ = style.print_line(
        "提示: /api-base set <url> 可改 OpenAI 兼容网关根地址（仅内存；别名 /apibase）。",
    );
    ReplSlashHandled::Handled
}

async fn slash_api_base_set(
    url: String,
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let t = url.trim();
    if t.is_empty() {
        let _ = style.eprint_error("用法: /api-base set <url>（例如 https://api.openai.com/v1）");
    } else if t.len() > REPL_LLM_API_BASE_MAX {
        let _ = style.eprint_error(&format!(
            "api_base 过长（上限 {REPL_LLM_API_BASE_MAX} 字符）。"
        ));
    } else if t.contains('\0') || t.contains('\r') || t.contains('\n') {
        let _ = style.eprint_error("api_base 含非法控制字符，已拒绝。");
    } else {
        let label = t.to_string();
        let mut w = cfg_holder.write().await;
        w.llm.api_base.clone_from(&label);
        refresh_llm_reasoning_split_for_gateway(&mut w);
        drop(w);
        let _ = style.print_success(&format!(
            "已设 api_base = {label}（仅本进程；持久化请改配置；/config reload 会从磁盘覆盖；llm_reasoning_split 已按网关默认刷新）"
        ));
    }
    ReplSlashHandled::Handled
}

fn slash_api_base_usage(style: &CliReplStyle) -> ReplSlashHandled {
    let _ = style.eprint_error("用法: /api-base（显示当前）· /api-base set <url>（写内存）");
    ReplSlashHandled::Handled
}

async fn slash_config(
    extra: &str,
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    tools: &[crate::types::Tool],
    style: &CliReplStyle,
    no_stream: bool,
) -> ReplSlashHandled {
    let e = extra.trim();
    if e.eq_ignore_ascii_case("reload") {
        return ReplSlashHandled::RunConfigReload;
    }
    if !e.is_empty() {
        let _ = style.eprint_error("用法: /config · /config reload（热重载，见文档）");
        return ReplSlashHandled::Handled;
    }
    let cfg = cfg_holder.read().await;
    if let Err(err) = style.print_repl_config_summary(&cfg, work_dir, tools.len(), no_stream) {
        let _ = style.eprint_error(&err.to_string());
    }
    ReplSlashHandled::Handled
}

async fn slash_doctor(
    extra: &str,
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    if !extra.is_empty() {
        let _ = style.eprint_error("用法: /doctor（无额外参数；同 crabmate doctor）");
    } else {
        let ws = work_dir.to_str();
        let cfg = cfg_holder.read().await;
        crate::runtime::cli_doctor::print_doctor_report(&cfg, ws);
    }
    ReplSlashHandled::Handled
}

fn slash_probe(extra: &str, style: &CliReplStyle) -> ReplSlashHandled {
    if !extra.is_empty() {
        let _ = style.eprint_error("用法: /probe（无额外参数；同 crabmate probe）");
        ReplSlashHandled::Handled
    } else {
        ReplSlashHandled::RunProbe
    }
}

fn slash_workspace_show(work_dir: &Path, style: &CliReplStyle) -> ReplSlashHandled {
    match work_dir.canonicalize() {
        Ok(p) => {
            let _ = style.print_line(&format!("当前工作区: {}", p.display()));
        }
        Err(_) => {
            let _ = style.print_line(&format!("当前工作区: {}", work_dir.display()));
        }
    }
    ReplSlashHandled::Handled
}

#[allow(clippy::ptr_arg)]
async fn slash_workspace_set(
    cfg_holder: &SharedAgentConfig,
    work_dir: &mut std::path::PathBuf,
    arg: &str,
    style: &CliReplStyle,
) -> ReplSlashHandled {
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
    ReplSlashHandled::Handled
}

async fn slash_skills_list(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if !cfg.skills.skills_enabled {
        let _ = style.print_line("skills 已关闭（skills_enabled=false）。");
        return ReplSlashHandled::Handled;
    }
    match crate::config::skills::list_skills_from_base(work_dir, &cfg.skills.skills_dir) {
        Ok(files) if files.is_empty() => {
            let _ = style.print_line("当前未发现 skills。");
        }
        Ok(files) => {
            let _ = style.print_line(&format!("当前 skills（{}）：", files.len()));
            for f in files {
                let name = f.name.as_deref().unwrap_or("未声明 name");
                let _ = style.print_line(&format!("  - {} (name: {})", f.display_path, name));
            }
        }
        Err(e) => {
            let _ = style.eprint_error(&format!("读取 skills 失败：{e}"));
        }
    }
    ReplSlashHandled::Handled
}

fn slash_tools_list(tools: &[crate::types::Tool], style: &CliReplStyle) -> ReplSlashHandled {
    if tools.is_empty() {
        let _ = style.print_line("当前未加载工具（可能使用了 --no-tools）。");
    } else {
        let _ = style.print_line(&format!("当前 {} 个工具:", tools.len()));
        for t in tools {
            let _ = style.print_line(&format!("  · {}", t.function.name));
        }
    }
    ReplSlashHandled::Handled
}

fn slash_export(
    arg: &str,
    work_dir: &Path,
    messages: &[Message],
    style: &CliReplStyle,
) -> ReplSlashHandled {
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
    ReplSlashHandled::Handled
}

async fn slash_save_session(
    arg: &str,
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let kind = match repl_export_kind_from_arg(arg) {
        Ok(k) => k,
        Err(()) => {
            let _ =
                style.eprint_error("用法: /save-session 或 /save-session json | markdown | both");
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
    ReplSlashHandled::Handled
}

async fn slash_agent_list(
    cfg_holder: &SharedAgentConfig,
    agent_role: &Option<String>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if cfg.roles_prompts.agent_roles.is_empty() {
        let _ = style.print_line(
            "当前配置未启用多角色（agent_roles 为空）。可在配置中加入 [[agent_roles]] 或 config/agent_roles.toml。",
        );
    } else {
        let mut ids: Vec<&String> = cfg.roles_prompts.agent_roles.keys().collect();
        ids.sort();
        let def = cfg.roles_prompts.default_agent_role_id.as_deref();
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
    ReplSlashHandled::Handled
}

async fn slash_agent_set(
    id: String,
    cfg_holder: &SharedAgentConfig,
    messages: &mut [Message],
    agent_role: &mut Option<String>,
    style: &CliReplStyle,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if cfg.roles_prompts.agent_roles.is_empty() {
        let _ = style.eprint_error(
            "当前未配置多角色，无法 /agent set。请先配置 [[agent_roles]] 或 agent_roles.toml。",
        );
    } else if repl_agent_role_set_is_default_pseudo(id.as_str()) {
        drop(cfg);
        *agent_role = None;
        let cfg = cfg_holder.read().await.clone();
        if let Err(e) = apply_agent_role_switch_to_messages(&cfg, messages, None, tool_recorder) {
            let _ = style.eprint_error(&e);
        } else {
            let _ = style.print_success(&format!(
                "已设回 default（清除显式命名角色），已更新首条 system（保留对话 {} 条）。",
                messages.len()
            ));
        }
    } else if let Err(e) = cfg.system_prompt_for_new_conversation(Some(id.as_str())) {
        let _ = style.eprint_error(&e);
    } else {
        let role_label = id.clone();
        drop(cfg);
        *agent_role = Some(id);
        let cfg = cfg_holder.read().await.clone();
        if let Err(e) = apply_agent_role_switch_to_messages(
            &cfg,
            messages,
            Some(role_label.as_str()),
            tool_recorder,
        ) {
            let _ = style.eprint_error(&e);
        } else {
            let _ = style.print_success(&format!(
                "已设当前角色为 \"{role_label}\"，已更新首条 system（保留对话 {} 条）。",
                messages.len()
            ));
        }
    }
    ReplSlashHandled::Handled
}

fn slash_api_key_usage(style: &CliReplStyle) -> ReplSlashHandled {
    let _ = style.print_line(
        "用法: /api-key status（是否已在本进程设置密钥）· /api-key set <密钥> · /api-key clear",
    );
    let _ = style.print_line(
        "说明: 密钥仅存本进程内存，不写盘；未设置环境变量 API_KEY 时可用此命令。/config reload 不会清除此处设置的值。",
    );
    ReplSlashHandled::Handled
}

async fn slash_api_key_status(
    cfg_holder: &SharedAgentConfig,
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let g = cfg_holder.read().await;
    let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
    let set = !k.trim().is_empty();
    drop(k);
    if g.llm.llm_http_auth_mode == LlmHttpAuthMode::None {
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
    ReplSlashHandled::Handled
}

fn slash_api_key_clear(
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    api_key_holder
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    let _ = style.print_success("已清除本进程内存中的 LLM API 密钥（环境变量 API_KEY 不受影响）。");
    ReplSlashHandled::Handled
}

fn slash_api_key_set(
    secret: String,
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    if secret.len() > 16384 {
        let _ = style.eprint_error("密钥过长（上限 16384 字符）。");
    } else {
        *api_key_holder.lock().unwrap_or_else(|e| e.into_inner()) = secret;
        let _ = style.print_success("已写入本进程 LLM API 密钥（仅存内存；值已隐藏）。");
    }
    ReplSlashHandled::Handled
}
