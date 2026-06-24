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
    /// 全屏 TUI：不向 stdout 刷助手输出（仍可按 `no_stream` SSE 拉取）。
    pub suppress_stdout_render: bool,
    /// 与 TUI 共用：流式增量缓冲；普通 `chat` / `repl` 为 `None`。
    pub tui_llm_stream_scratch: Option<crate::runtime::tui::TuiLlmStreamScratchArc>,
    /// 无 SSE 时工具批状态回调（TUI 底栏）；通常为 `None`。
    pub tool_running_hook: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    /// TUI：工具 `present_clarification_questionnaire` 成功时通知 UI；`repl` / `chat` 为 `None`。
    pub clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub cli_tool_ctx: Option<&'a CliToolRuntime>,
    pub active_agent_role: Option<&'a str>,
    pub process_handles: Arc<ProcessHandles>,
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
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
        suppress_stdout_render,
        tui_llm_stream_scratch,
        tool_running_hook,
        clarification_questionnaire_hook,
        cli_tool_ctx,
        active_agent_role,
        process_handles,
        sse_control_mirror,
    } = p;
    let (ltm, scope) = process_handles
        .cli_long_term_memory_handles_with_stderr_notice(cfg, &CLI_LTM_OPEN_FAILURE_NOTIFIED);
    let turn_allow = turn_allow_for_web_or_cli_job(cfg, active_agent_role, None);
    let tools_for_job = filter_tools_for_agent_role(tools, turn_allow.as_ref().map(|a| a.as_ref()));
    run_agent_turn(RunAgentTurnParams::cli_terminal_chat(
        crate::CliTerminalChatBuildArgs {
            shared: crate::RunAgentTurnSharedInputs {
                client,
                api_key,
                cfg,
                tools: tools_for_job.as_slice(),
            },
            messages,
            effective_working_dir: work_dir,
            no_stream,
            suppress_stdout_render,
            tui_llm_stream_scratch,
            tool_running_hook,
            clarification_questionnaire_hook,
            cli_tool_ctx,
            long_term_memory: ltm.clone(),
            long_term_memory_scope_id: scope.clone(),
            turn_allowed_tool_names: turn_allow,
            process_handles,
            sse_control_mirror,
        },
    ))
    .await
    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    if let (Some(rt), Some(scope_id)) = (ltm, scope) {
        rt.spawn_turn_memory_postprocess(Arc::clone(cfg), scope_id, messages.clone());
    }
    Ok(())
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
        tui_blocking_approval_tx: None,
    }
}

fn resolve_system_prompt_for_chat(
    cfg: &Arc<AgentConfig>,
    chat: &ChatCliArgs,
    agent_role: Option<&str>,
    tool_recorder: &std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
    work_dir: &Path,
    user_text_for_skills: Option<&str>,
) -> Result<
    (
        String,
        crate::context_bootstrap::prompt_compose::FirstSystemComposeDiagnostics,
    ),
    Box<dyn std::error::Error>,
> {
    if let Some(p) = chat.system_prompt_file.as_deref() {
        let base = std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --system-prompt-file {p}: {e}"),
            )
        })?;
        let l4 = crate::context_bootstrap::prompt_compose::compose_system_from_base(
            &base,
            cfg,
            tool_recorder,
            None,
        );
        let (final_prompt, skills_meta) = if let Some(user_text) = user_text_for_skills
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            crate::config::skills::merge_system_prompt_with_skills_selected_with_meta(
                l4.clone(),
                cfg.skills.skills_enabled,
                cfg.skills.skills_dir.as_str(),
                cfg.skills.skills_max_chars,
                work_dir,
                user_text,
                cfg.skills.skills_top_k,
            )
            .unwrap_or((
                l4.clone(),
                crate::config::skills::SkillsSelectionMeta::default(),
            ))
        } else {
            (
                l4.clone(),
                crate::config::skills::SkillsSelectionMeta::default(),
            )
        };
        let mut layers = vec!["L3".to_string(), "L4".to_string()];
        if !skills_meta.selected_labels.is_empty() {
            layers.push("L5".to_string());
        }
        return Ok((
            final_prompt.clone(),
            crate::context_bootstrap::prompt_compose::FirstSystemComposeDiagnostics {
                layers_applied: layers,
                chars_l3_base: base.chars().count(),
                chars_l4_augmented: l4.chars().count(),
                chars_final: final_prompt.chars().count(),
                skills_total_docs: skills_meta.total_docs,
                skills_selected_labels: skills_meta.selected_labels,
            },
        ));
    }
    let role = crate::context_bootstrap::prompt_compose::resolve_agent_role_for_prompt_compose(
        cfg, agent_role, None,
    )
    .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
    crate::context_bootstrap::prompt_compose::compose_first_system_for_turn_with_diagnostics(
        cfg,
        tool_recorder,
        crate::context_bootstrap::prompt_compose::FirstSystemComposeOpts {
            agent_role: role.as_deref(),
            user_msg_for_skills: user_text_for_skills,
            skills_base_dir: Some(work_dir.to_path_buf()),
            role_resolution: crate::context_bootstrap::prompt_compose::RoleSystemResolution::Strict,
        },
    )
    .map_err(|e| CliExitError::new(EXIT_USAGE, e).into())
}

fn log_first_system_diagnostics(
    path: &str,
    diag: &crate::context_bootstrap::prompt_compose::FirstSystemComposeDiagnostics,
) {
    debug!(
        target: "crabmate",
        "first_system_compose path={} layers={:?} chars_l3={} chars_l4={} chars_final={} skills_total={} skills_selected={:?}",
        path,
        diag.layers_applied,
        diag.chars_l3_base,
        diag.chars_l4_augmented,
        diag.chars_final,
        diag.skills_total_docs,
        diag.skills_selected_labels
    );
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

struct ChatBatchLineMergeCtx<'a> {
    cfg_holder: &'a SharedAgentConfig,
    chat: &'a ChatCliArgs,
    agent_role: Option<&'a str>,
    process_handles: &'a ProcessHandles,
    work_dir: &'a Path,
    path: &'a str,
}

/// 将 JSONL 单行对象合并进 `messages`（`user` 或 `messages` 分支）；从 `run_chat_batch_jsonl` 拆出以降低圈复杂度。
async fn chat_batch_jsonl_merge_line_value(
    ctx: &ChatBatchLineMergeCtx<'_>,
    messages: &mut Vec<Message>,
    line_no: usize,
    v: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let ChatBatchLineMergeCtx {
        cfg_holder,
        chat,
        agent_role,
        process_handles,
        work_dir,
        path,
    } = *ctx;
    if let Some(u) = v.get("user").and_then(|x| x.as_str()) {
        let u = u.trim();
        if u.is_empty() {
            return Err(CliExitError::new(
                EXIT_USAGE,
                format!("{path} 第 {line_no} 行：user 为空"),
            )
            .into());
        }
        let cfg_snap = {
            let g = cfg_holder.read().await;
            Arc::new(g.clone())
        };
        let u_exp = expand_at_file_refs_in_user_message(u, work_dir, cfg_snap.as_ref())
            .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
        if messages.is_empty() {
            let (system_selected, diag) = resolve_system_prompt_for_chat(
                &cfg_snap,
                chat,
                agent_role,
                &process_handles.tool_outcome_recorder,
                work_dir,
                Some(u_exp.as_str()),
            )?;
            log_first_system_diagnostics("cli_chat_batch_first_turn", &diag);
            *messages = messages_chat_seed(&system_selected, &u_exp);
            prepend_cli_first_turn_injection(cfg_holder, work_dir, messages).await;
        } else {
            messages.push(Message::user_only(u_exp));
        }
        return Ok(());
    }
    if let Some(m) = v.get("messages") {
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
        *messages = normalize_messages_for_openai_compatible_request(parsed);
        return Ok(());
    }
    Err(CliExitError::new(
        EXIT_USAGE,
        format!("{path} 第 {line_no} 行：需要字段 `user`（字符串）或 `messages`（数组）"),
    )
    .into())
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
    let mut messages: Vec<Message> = Vec::new();
    let mut line_no: usize = 0;
    let merge_ctx = ChatBatchLineMergeCtx {
        cfg_holder,
        chat,
        agent_role,
        process_handles: process_handles.as_ref(),
        work_dir,
        path,
    };
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
        chat_batch_jsonl_merge_line_value(&merge_ctx, &mut messages, line_no, &v).await?;

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
            suppress_stdout_render: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            cli_tool_ctx: Some(cli_rt),
            active_agent_role: agent_role,
            process_handles: Arc::clone(&process_handles),
            sse_control_mirror: None,
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

async fn chat_invocation_validate_role_and_system_prompt(
    cfg_holder: &SharedAgentConfig,
    chat: &ChatCliArgs,
    agent_role: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}

/// `chat` 子命令多路径共用的 LLM 调用上下文（减少 `run_chat_invocation` 拆分函数的形参个数）。
struct ChatInvocationTurnCtx<'a> {
    cfg_holder: &'a SharedAgentConfig,
    client: &'a reqwest::Client,
    api_key: &'a str,
    tools: &'a [crate::types::Tool],
    work_dir: &'a std::path::Path,
    chat: &'a ChatCliArgs,
    agent_role: Option<&'a str>,
    process_handles: Arc<ProcessHandles>,
    cli_rt: &'a CliToolRuntime,
    json_out: bool,
}

async fn chat_invocation_via_messages_json_file(
    ctx: &ChatInvocationTurnCtx<'_>,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut messages = load_messages_json_file(path)?;
    debug!(
        target: "crabmate::print",
        "messages-json-file 已加载 path={} count={}",
        path,
        messages.len()
    );
    let cfg_snap = {
        let g = ctx.cfg_holder.read().await;
        Arc::new(g.clone())
    };
    run_agent_turn_for_cli(RunAgentTurnForCliParams {
        client: ctx.client,
        api_key: ctx.api_key,
        cfg: &cfg_snap,
        tools: ctx.tools,
        messages: &mut messages,
        work_dir: ctx.work_dir,
        no_stream: ctx.chat.no_stream,
        suppress_stdout_render: false,
        tui_llm_stream_scratch: None,
        tool_running_hook: None,
        clarification_questionnaire_hook: None,
        cli_tool_ctx: Some(ctx.cli_rt),
        active_agent_role: ctx.agent_role,
        process_handles: Arc::clone(&ctx.process_handles),
        sse_control_mirror: None,
    })
    .await
    .map_err(map_turn_err)?;
    ensure_all_run_commands_not_denied(ctx.cli_rt)?;
    if ctx.json_out {
        print_json_reply_line(&cfg_snap, &messages, None);
    }
    Ok(())
}

async fn chat_invocation_via_cli_query(
    ctx: ChatInvocationTurnCtx<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let user = resolve_user_body(ctx.chat)?;
    let cfg_for_expand = {
        let g = ctx.cfg_holder.read().await;
        Arc::new(g.clone())
    };
    let user = expand_at_file_refs_in_user_message(&user, ctx.work_dir, cfg_for_expand.as_ref())
        .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
    let (system, diag) = resolve_system_prompt_for_chat(
        &cfg_for_expand,
        ctx.chat,
        ctx.agent_role,
        &ctx.process_handles.tool_outcome_recorder,
        ctx.work_dir,
        Some(user.as_str()),
    )?;
    log_first_system_diagnostics("cli_chat_query_first_turn", &diag);
    let mut messages = messages_chat_seed(&system, &user);
    prepend_cli_first_turn_injection(ctx.cfg_holder, ctx.work_dir, &mut messages).await;
    debug!(
        target: "crabmate::print",
        "chat 首轮已构造 system_len={} user_preview={}",
        system.len(),
        redact::preview_chars(&user, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let cfg_snap = {
        let g = ctx.cfg_holder.read().await;
        Arc::new(g.clone())
    };
    run_agent_turn_for_cli(RunAgentTurnForCliParams {
        client: ctx.client,
        api_key: ctx.api_key,
        cfg: &cfg_snap,
        tools: ctx.tools,
        messages: &mut messages,
        work_dir: ctx.work_dir,
        no_stream: ctx.chat.no_stream,
        suppress_stdout_render: false,
        tui_llm_stream_scratch: None,
        tool_running_hook: None,
        clarification_questionnaire_hook: None,
        cli_tool_ctx: Some(ctx.cli_rt),
        active_agent_role: ctx.agent_role,
        process_handles: ctx.process_handles,
        sse_control_mirror: None,
    })
    .await
    .map_err(map_turn_err)?;
    ensure_all_run_commands_not_denied(ctx.cli_rt)?;
    if ctx.json_out {
        print_json_reply_line(&cfg_snap, &messages, None);
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
    chat_invocation_validate_role_and_system_prompt(cfg_holder, chat, agent_role).await?;
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
        let ctx = ChatInvocationTurnCtx {
            cfg_holder,
            client,
            api_key,
            tools,
            work_dir: work_dir.as_path(),
            chat,
            agent_role,
            process_handles: Arc::clone(&process_handles),
            cli_rt: &cli_rt,
            json_out,
        };
        return chat_invocation_via_messages_json_file(&ctx, path).await;
    }

    chat_invocation_via_cli_query(ChatInvocationTurnCtx {
        cfg_holder,
        client,
        api_key,
        tools,
        work_dir: work_dir.as_path(),
        chat,
        agent_role,
        process_handles,
        cli_rt: &cli_rt,
        json_out,
    })
    .await
}
