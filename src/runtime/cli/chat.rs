//! `chat` 子命令：`--query` / JSONL / `--messages-json-file` 等。

use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};
use crate::config::cli::ChatCliArgs;
use crate::config::{AgentConfig, SharedAgentConfig};
use crate::process_handles::ProcessHandles;
use crate::redact;
use crate::runtime::cli::cli_effective_work_dir;
use crate::runtime::cli::repl_extras::prepend_cli_first_turn_injection;
use crate::runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
    classify_model_error_message,
};
use crate::tool_registry::{CliCommandTurnStats, CliToolRuntime};
use crate::types::{Message, messages_chat_seed, normalize_messages_for_openai_compatible_request};
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use crate::{RunAgentTurnParams, run_agent_turn};
use log::debug;
use std::collections::HashSet;
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

/// 长期记忆库打开失败时，仅向 stderr 打印**一次**用户可见说明（避免每轮 REPL/chat 重复刷屏）。
static CLI_LTM_OPEN_FAILURE_NOTIFIED: AtomicBool = AtomicBool::new(false);

/// `cli_run` → `run_chat_invocation` / `run_repl` 共用的入口参数（合并以减少顶层形参个数）。
pub struct CliMainInvocationCommon<'a> {
    pub cfg_holder: &'a SharedAgentConfig,
    pub config_path: Option<&'a str>,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub tools: &'a [crate::types::Tool],
    pub workspace_cli: &'a Option<String>,
    pub agent_role: Option<&'a str>,
    pub process_handles: Arc<ProcessHandles>,
}

/// CLI（无 SSE、`workspace_is_set` 恒为真）下调用 [`run_agent_turn`] 的固定参数封装。
pub(crate) struct RunAgentTurnForCliParams<'a> {
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub work_dir: &'a std::path::Path,
    pub no_stream: bool,
    pub cli_tool_ctx: Option<&'a CliToolRuntime>,
    pub active_agent_role: Option<&'a str>,
    pub process_handles: Arc<ProcessHandles>,
}

pub(crate) async fn run_agent_turn_for_cli(
    p: RunAgentTurnForCliParams<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let RunAgentTurnForCliParams {
        client,
        api_key,
        cfg,
        tools,
        messages,
        work_dir,
        no_stream,
        cli_tool_ctx,
        active_agent_role,
        process_handles,
    } = p;
    let (ltm, scope) = process_handles
        .cli_long_term_memory_handles_with_stderr_notice(cfg, &CLI_LTM_OPEN_FAILURE_NOTIFIED);
    let turn_allow = turn_allow_for_web_or_cli_job(cfg, active_agent_role, None);
    let tools_for_job = filter_tools_for_agent_role(tools, turn_allow.as_ref().map(|a| a.as_ref()));
    run_agent_turn(RunAgentTurnParams::cli_terminal_chat(
        crate::CliTerminalChatBuildArgs {
            client,
            api_key,
            cfg,
            tools: tools_for_job.as_slice(),
            messages,
            effective_working_dir: work_dir,
            no_stream,
            cli_tool_ctx,
            long_term_memory: ltm,
            long_term_memory_scope_id: scope,
            turn_allowed_tool_names: turn_allow,
            process_handles,
        },
    ))
    .await
    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
}

fn map_turn_err(e: Box<dyn std::error::Error + Send + Sync>) -> Box<dyn std::error::Error> {
    let s = e.to_string();
    let code = classify_model_error_message(&s);
    Box::new(CliExitError::new(code, s))
}

fn build_cli_runtime(chat: &ChatCliArgs) -> CliToolRuntime {
    let extra: Vec<String> = chat
        .approve_commands
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    CliToolRuntime {
        persistent_allowlist_shared: Arc::new(Mutex::new(HashSet::new())),
        auto_approve_all_non_whitelist_run_command: chat.yes_run_command,
        extra_allowlist_commands: extra.into(),
        command_stats: Arc::new(std::sync::Mutex::new(CliCommandTurnStats::default())),
    }
}

fn resolve_system_prompt_for_chat(
    cfg: &Arc<AgentConfig>,
    chat: &ChatCliArgs,
    agent_role: Option<&str>,
    tool_recorder: &std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
) -> Result<String, Box<dyn std::error::Error>> {
    let base = if let Some(p) = chat.system_prompt_file.as_deref() {
        std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --system-prompt-file {p}: {e}"),
            )
        })?
    } else {
        cfg.system_prompt_for_new_conversation(agent_role)
            .map_err(|e| CliExitError::new(EXIT_USAGE, e))?
            .to_string()
    };
    Ok(tool_recorder.augment_system_prompt(&base, cfg))
}

fn resolve_user_body(chat: &ChatCliArgs) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = chat.user_prompt_file.as_deref() {
        let t = std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --user-prompt-file {p}: {e}"),
            )
        })?;
        let t = t.trim();
        if t.is_empty() {
            return Err(CliExitError::new(
                EXIT_USAGE,
                "--user-prompt-file 文件内容为空（去空白后）",
            )
            .into());
        }
        return Ok(t.to_string());
    }
    let Some(u) = chat.inline_user_text.as_deref() else {
        return Err(CliExitError::new(EXIT_USAGE, "缺少用户消息").into());
    };
    let u = u.trim();
    if u.is_empty() {
        return Err(
            CliExitError::new(EXIT_USAGE, "--query 或 --stdin 用户内容为空（去空白后）").into(),
        );
    }
    Ok(u.to_string())
}

fn load_messages_json_file(path: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        CliExitError::new(
            EXIT_GENERAL,
            format!("无法读取 --messages-json-file {path}: {e}"),
        )
    })?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| CliExitError::new(EXIT_USAGE, format!("{path}：顶层 JSON 解析失败: {e}")))?;
    let parsed: Vec<Message> = if let Some(a) = v.as_array() {
        serde_json::from_value(serde_json::Value::Array(a.clone())).map_err(|e| {
            CliExitError::new(EXIT_USAGE, format!("{path}：消息数组反序列化失败: {e}"))
        })?
    } else if let Some(m) = v.get("messages") {
        serde_json::from_value(m.clone()).map_err(|e| {
            CliExitError::new(
                EXIT_USAGE,
                format!("{path}：messages 字段反序列化失败: {e}"),
            )
        })?
    } else {
        return Err(CliExitError::new(
            EXIT_USAGE,
            format!("{path}：须为 JSON 数组，或对象 {{\"messages\":[...]}}"),
        )
        .into());
    };
    if parsed.is_empty() {
        return Err(CliExitError::new(EXIT_USAGE, format!("{path}：messages 不能为空")).into());
    }
    Ok(normalize_messages_for_openai_compatible_request(parsed))
}

fn print_json_reply_line(cfg: &Arc<AgentConfig>, messages: &[Message], batch_line: Option<usize>) {
    let reply = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| crate::types::message_content_into_text_lossy(m.content.clone()))
        .unwrap_or_default();
    let mut obj = serde_json::json!({
        "type": "crabmate_chat_cli_result",
        "v": 1u32,
        "reply": reply,
        "model": cfg.llm.model,
    });
    if let Some(n) = batch_line {
        obj["batch_line"] = serde_json::json!(n);
    }
    println!("{}", obj);
}

fn ensure_all_run_commands_not_denied(
    cli_rt: &CliToolRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    if cli_rt.all_run_commands_were_denied() {
        return Err(Box::new(CliExitError::new(
            EXIT_TOOLS_ALL_RUN_COMMAND_DENIED,
            "本回合内所有 run_command 均在审批中被拒绝（或未自动批准）",
        )));
    }
    Ok(())
}

struct RunChatBatchJsonlParams<'a> {
    cfg_holder: &'a SharedAgentConfig,
    _config_path: Option<&'a str>,
    client: &'a reqwest::Client,
    api_key: &'a str,
    tools: &'a [crate::types::Tool],
    work_dir: &'a Path,
    no_stream: bool,
    cli_rt: &'a CliToolRuntime,
    json_out: bool,
    path: &'a str,
    chat: &'a ChatCliArgs,
    agent_role: Option<&'a str>,
    process_handles: Arc<ProcessHandles>,
}

async fn run_chat_batch_jsonl(
    p: RunChatBatchJsonlParams<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let RunChatBatchJsonlParams {
        cfg_holder,
        _config_path,
        client,
        api_key,
        tools,
        work_dir,
        no_stream,
        cli_rt,
        json_out,
        path,
        chat,
        agent_role,
        process_handles,
    } = p;
    let file = std::fs::File::open(path).map_err(|e| {
        CliExitError::new(EXIT_GENERAL, format!("无法打开 --message-file {path}: {e}"))
    })?;
    let reader = std::io::BufReader::new(file);
    let system_seed = {
        let g = cfg_holder.read().await;
        resolve_system_prompt_for_chat(
            &Arc::new(g.clone()),
            chat,
            agent_role,
            &process_handles.tool_outcome_recorder,
        )?
    };
    let mut messages: Vec<Message> = Vec::new();
    let mut line_no: usize = 0;
    for line in reader.lines() {
        line_no += 1;
        let line = line.map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("读取 {path} 第 {line_no} 行失败: {e}"),
            )
        })?;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(t).map_err(|e| {
            CliExitError::new(
                EXIT_USAGE,
                format!("{path} 第 {line_no} 行 JSON 解析失败: {e}"),
            )
        })?;
        if let Some(u) = v.get("user").and_then(|x| x.as_str()) {
            let u = u.trim();
            if u.is_empty() {
                return Err(CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：user 为空"),
                )
                .into());
            }
            if messages.is_empty() {
                let cfg_snap = {
                    let g = cfg_holder.read().await;
                    Arc::new(g.clone())
                };
                let u_exp = expand_at_file_refs_in_user_message(u, work_dir, cfg_snap.as_ref())
                    .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
                let system_selected =
                    crate::config::skills::merge_system_prompt_with_skills_selected(
                        system_seed.clone(),
                        cfg_snap.skills.skills_enabled,
                        cfg_snap.skills.skills_dir.as_str(),
                        cfg_snap.skills.skills_max_chars,
                        work_dir,
                        &u_exp,
                        cfg_snap.skills.skills_top_k,
                    )
                    .unwrap_or_else(|_| system_seed.clone());
                messages = messages_chat_seed(&system_selected, &u_exp);
                prepend_cli_first_turn_injection(cfg_holder, work_dir, &mut messages).await;
            } else {
                let cfg_snap = {
                    let g = cfg_holder.read().await;
                    Arc::new(g.clone())
                };
                let u_exp = expand_at_file_refs_in_user_message(u, work_dir, cfg_snap.as_ref())
                    .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
                messages.push(Message::user_only(u_exp));
            }
        } else if let Some(m) = v.get("messages") {
            let parsed: Vec<Message> = serde_json::from_value(m.clone()).map_err(|e| {
                CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：messages 非法: {e}"),
                )
            })?;
            if parsed.is_empty() {
                return Err(CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：messages 为空"),
                )
                .into());
            }
            messages = normalize_messages_for_openai_compatible_request(parsed);
        } else {
            return Err(CliExitError::new(
                EXIT_USAGE,
                format!("{path} 第 {line_no} 行：需要字段 `user`（字符串）或 `messages`（数组）"),
            )
            .into());
        }

        let cfg_snap = {
            let g = cfg_holder.read().await;
            Arc::new(g.clone())
        };
        run_agent_turn_for_cli(RunAgentTurnForCliParams {
            client,
            api_key,
            cfg: &cfg_snap,
            tools,
            messages: &mut messages,
            work_dir,
            no_stream,
            cli_tool_ctx: Some(cli_rt),
            active_agent_role: agent_role,
            process_handles: Arc::clone(&process_handles),
        })
        .await
        .map_err(map_turn_err)?;
        ensure_all_run_commands_not_denied(cli_rt)?;
        if json_out {
            print_json_reply_line(&cfg_snap, &messages, Some(line_no));
        }
    }
    Ok(())
}

pub async fn run_chat_invocation(
    common: CliMainInvocationCommon<'_>,
    chat: &ChatCliArgs,
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
    let work_dir = {
        let g = cfg_holder.read().await;
        cli_effective_work_dir(workspace_cli, &g.command_exec.run_command_working_dir)
    };
    {
        let g = cfg_holder.read().await;
        if agent_role.is_some() && chat.system_prompt_file.is_some() {
            return Err(CliExitError::new(
                EXIT_USAGE,
                "--agent-role 与 --system-prompt-file 不能同时使用",
            )
            .into());
        }
        if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            g.system_prompt_for_new_conversation(Some(r))
                .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
        }
    }
    let cli_rt = build_cli_runtime(chat);
    let json_out = chat.output.as_deref().is_some_and(|m| m == "json");

    if let Some(batch_path) = chat.message_file.as_deref() {
        return run_chat_batch_jsonl(RunChatBatchJsonlParams {
            cfg_holder,
            _config_path: config_path,
            client,
            api_key,
            tools,
            work_dir: work_dir.as_path(),
            no_stream: chat.no_stream,
            cli_rt: &cli_rt,
            json_out,
            path: batch_path,
            chat,
            agent_role,
            process_handles: Arc::clone(&process_handles),
        })
        .await;
    }

    if let Some(path) = chat.messages_json_file.as_deref() {
        let mut messages = load_messages_json_file(path)?;
        debug!(
            target: "crabmate::print",
            "messages-json-file 已加载 path={} count={}",
            path,
            messages.len()
        );
        let cfg_snap = {
            let g = cfg_holder.read().await;
            Arc::new(g.clone())
        };
        run_agent_turn_for_cli(RunAgentTurnForCliParams {
            client,
            api_key,
            cfg: &cfg_snap,
            tools,
            messages: &mut messages,
            work_dir: work_dir.as_path(),
            no_stream: chat.no_stream,
            cli_tool_ctx: Some(&cli_rt),
            active_agent_role: agent_role,
            process_handles: Arc::clone(&process_handles),
        })
        .await
        .map_err(map_turn_err)?;
        ensure_all_run_commands_not_denied(&cli_rt)?;
        if json_out {
            print_json_reply_line(&cfg_snap, &messages, None);
        }
        return Ok(());
    }

    let system = {
        let g = cfg_holder.read().await;
        resolve_system_prompt_for_chat(
            &Arc::new(g.clone()),
            chat,
            agent_role,
            &process_handles.tool_outcome_recorder,
        )?
    };
    let user = resolve_user_body(chat)?;
    let cfg_for_expand = {
        let g = cfg_holder.read().await;
        Arc::new(g.clone())
    };
    let user =
        expand_at_file_refs_in_user_message(&user, work_dir.as_path(), cfg_for_expand.as_ref())
            .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
    let base_system = system;
    let system = crate::config::skills::merge_system_prompt_with_skills_selected(
        base_system.clone(),
        cfg_for_expand.skills.skills_enabled,
        cfg_for_expand.skills.skills_dir.as_str(),
        cfg_for_expand.skills.skills_max_chars,
        work_dir.as_path(),
        &user,
        cfg_for_expand.skills.skills_top_k,
    )
    .unwrap_or(base_system);
    let mut messages = messages_chat_seed(&system, &user);
    prepend_cli_first_turn_injection(cfg_holder, work_dir.as_path(), &mut messages).await;
    debug!(
        target: "crabmate::print",
        "chat 首轮已构造 system_len={} user_preview={}",
        system.len(),
        redact::preview_chars(&user, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let cfg_snap = {
        let g = cfg_holder.read().await;
        Arc::new(g.clone())
    };
    run_agent_turn_for_cli(RunAgentTurnForCliParams {
        client,
        api_key,
        cfg: &cfg_snap,
        tools,
        messages: &mut messages,
        work_dir: work_dir.as_path(),
        no_stream: chat.no_stream,
        cli_tool_ctx: Some(&cli_rt),
        active_agent_role: agent_role,
        process_handles,
    })
    .await
    .map_err(map_turn_err)?;
    ensure_all_run_commands_not_denied(&cli_rt)?;
    if json_out {
        print_json_reply_line(&cfg_snap, &messages, None);
    }
    Ok(())
}
