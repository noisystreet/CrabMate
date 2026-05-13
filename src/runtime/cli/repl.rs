//! 交互式 REPL 主循环。

use crate::ProcessHandles;
use crate::clarification_questionnaire::merge_user_text_with_clarification_answers;
use crate::config::{LlmHttpAuthMode, SharedAgentConfig};
use crate::redact;
use crate::runtime::cli::chat::{
    CliMainInvocationCommon, RunAgentTurnForCliParams, run_agent_turn_for_cli,
};
use crate::runtime::cli::cli_effective_work_dir;
use crate::runtime::cli::repl_extras::{
    ReplSlashHandled, ReplSlashSharedHandles, try_handle_repl_slash_command,
};
use crate::runtime::cli::repl_parse::run_repl_shell_line_sync;
use crate::runtime::cli_exit::CliExitError;
use crate::runtime::cli_exit::EXIT_USAGE;
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::repl_reedline::{ReplLineEditor, ReplReadLine, read_repl_line_with_editor};
use crate::runtime::tui_terminal_bridge::{
    TuiTerminalHandoffOp, blocking_release_terminal, blocking_restore_terminal,
    pause_for_return_to_tui,
};
use crate::tool_registry::CliToolRuntime;
use crate::types::Message;
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use log::debug;

/// TUI 等：用户句已入队 `messages` 后的即时刷新回调（不等整轮模型返回）。
pub(crate) type ReplAfterUserMessageEnqueuedCb = Arc<dyn Fn(&[Message]) + Send + Sync>;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

const REPL_SHELL_USAGE: &str = "bash#: <命令>  在当前工作区执行一行 shell（不发给模型；无交互 stdin）。等同本机 `sh -c` / `cmd /C`，不受模型 `run_command` 白名单约束，仅应在可信环境使用。交互 TTY：空行按 `$` 即切换「我:」/ bash#:（也可单独一行 `$` 后 Enter）；管道/非 TTY 仍可用行内 `$ <命令>`。历史保存在工作区 `.crabmate/repl_history.txt`。示例: ls  pwd  git status";

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
                        let _ = ctx.style.print_success(&format!(
                        "已设 model = {resolved}（仅本进程有效；持久化请改配置文件；/config reload 会从磁盘覆盖）"
                    ));
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
    } = p;
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

/// 处理普通对话输入（不含 `/` 斜杠命令与 quit）：合并后台上下文、校验密钥、展开 `@`、合并 system、跑一轮 agent。
/// 普通对话回合所需句柄（不含 `/` 斜杠命令分支）。
pub(crate) struct ReplDispatchChatRoundParams<'a> {
    pub(crate) input: String,
    pub(crate) cfg_holder: &'a SharedAgentConfig,
    pub(crate) tools: &'a [crate::types::Tool],
    pub(crate) messages: &'a mut Vec<Message>,
    pub(crate) work_dir: &'a mut Path,
    pub(crate) style: &'a CliReplStyle,
    pub(crate) no_stream: bool,
    pub(crate) suppress_stdout_render: bool,
    pub(crate) tui_llm_stream_scratch: Option<crate::runtime::tui::TuiLlmStreamScratchArc>,
    /// TUI：工具批开始/结束回调（底栏「工具执行中…」）；REPL 为 `None`。
    pub(crate) tool_running_hook: Option<std::sync::Arc<dyn Fn(bool) + Send + Sync>>,
    /// TUI 等：用户消息已写入 `messages` 后立即刷新展示（不等整轮 `run_agent_turn` 结束）。
    pub(crate) after_user_message_enqueued: Option<ReplAfterUserMessageEnqueuedCb>,
    pub(crate) agent_role_owned: &'a mut Option<String>,
    pub(crate) api_key_holder: &'a Arc<StdMutex<String>>,
    pub(crate) client: &'a reqwest::Client,
    pub(crate) cli_rt: &'a CliToolRuntime,
    pub(crate) initial_pending: Option<&'a Arc<StdMutex<Option<Vec<crate::types::Message>>>>>,
    pub(crate) process_handles: Arc<ProcessHandles>,
    /// TUI：问卷 Modal 提交后在下一轮并入用户正文（与 Web `clarify_questionnaire_answers` 对齐）。
    pub(crate) clarify_answers_for_next_user_message: Option<
        &'a Arc<StdMutex<Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>>>,
    >,
    /// TUI：`present_clarification_questionnaire` 回调；`repl` 为 `None`。
    pub(crate) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    /// TUI：SSE 控制面镜像；`repl` 为 `None`。
    pub(crate) sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

pub(crate) async fn repl_dispatch_chat_round(
    p: ReplDispatchChatRoundParams<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ReplDispatchChatRoundParams {
        input,
        cfg_holder,
        tools,
        messages,
        work_dir,
        style,
        no_stream,
        suppress_stdout_render,
        tui_llm_stream_scratch,
        tool_running_hook,
        after_user_message_enqueued,
        agent_role_owned,
        api_key_holder,
        client,
        cli_rt,
        initial_pending,
        process_handles,
        clarify_answers_for_next_user_message,
        clarification_questionnaire_hook,
        sse_control_mirror,
    } = p;
    crate::runtime::workspace_session::try_merge_background_initial_workspace(
        messages,
        initial_pending,
    );
    let expanded_user = {
        let g = cfg_holder.read().await;
        match expand_at_file_refs_in_user_message(input.as_str(), work_dir, &g) {
            Ok(s) => s,
            Err(e) => {
                let _ = style.eprint_error(&e);
                return Ok(());
            }
        }
    };
    let clarify_take = clarify_answers_for_next_user_message
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));
    let user_body = merge_user_text_with_clarification_answers(expanded_user, clarify_take);
    {
        let g = cfg_holder.read().await;
        if let Some(first) = messages.first_mut()
            && first.role == "system"
        {
            let base_system =
                crate::context_bootstrap::conversation_turn_bootstrap::augmented_system_for_new_conversation_lenient(
                    &g,
                    agent_role_owned.as_deref(),
                    &process_handles.tool_outcome_recorder,
                );
            let merged = crate::config::skills::merge_system_prompt_with_skills_selected(
                base_system.clone(),
                g.skills.skills_enabled,
                g.skills.skills_dir.as_str(),
                g.skills.skills_max_chars,
                work_dir,
                user_body.as_str(),
                g.skills.skills_top_k,
            )
            .unwrap_or(base_system);
            first.content = Some(crate::types::MessageContent::Text(merged));
        }
    }
    messages.push(Message::user_only(user_body));
    if let Some(cb) = after_user_message_enqueued.as_ref() {
        cb(messages.as_slice());
    }
    debug!(
        target: "crabmate::print",
        "REPL 用户输入已入队 history_len={} input_preview={}",
        messages.len(),
        redact::preview_chars(input.as_str(), redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    // 须在入队用户消息之后再拦截：否则 TUI 等仅依赖 `messages` 的界面看不到已发送输入。
    {
        let g = cfg_holder.read().await;
        if g.llm.llm_http_auth_mode == LlmHttpAuthMode::Bearer {
            let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
            if k.trim().is_empty() {
                drop(k);
                let _ = style.eprint_error(
                    "当前为 llm_http_auth_mode=bearer，但未配置 LLM API 密钥。请执行 /api-key set <密钥>（仅本进程）或设置环境变量 API_KEY 后重启。",
                );
                return Ok(());
            }
        }
    }

    let cfg_snap = {
        let g = cfg_holder.read().await;
        Arc::new(g.clone())
    };
    let key_snap = api_key_holder
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Err(e) = run_agent_turn_for_cli(RunAgentTurnForCliParams {
        client,
        api_key: key_snap.as_str(),
        cfg: &cfg_snap,
        tools,
        messages,
        work_dir,
        no_stream,
        suppress_stdout_render,
        tui_llm_stream_scratch,
        tool_running_hook,
        clarification_questionnaire_hook,
        cli_tool_ctx: Some(cli_rt),
        active_agent_role: agent_role_owned.as_deref(),
        process_handles: Arc::clone(&process_handles),
        sse_control_mirror,
    })
    .await
    {
        let _ = style.eprint_error(&format!(
            "本轮对话失败（可继续输入；异常历史可 /clear 清空）：{}",
            e
        ));
    }
    Ok(())
}

/// 构建首轮消息（含可选后台扫描）、并打开 `.crabmate/repl_history.txt` 行编辑器。
pub(crate) async fn repl_prepare_messages_and_editor(
    cfg_holder: &SharedAgentConfig,
    tui_load: bool,
    work_dir: &Path,
    agent_role_owned: &Option<String>,
    run_root: &str,
    process_handles: Arc<ProcessHandles>,
) -> Result<
    (
        Vec<Message>,
        Option<Arc<StdMutex<Option<Vec<Message>>>>>,
        Arc<StdMutex<ReplLineEditor>>,
    ),
    Box<dyn std::error::Error>,
> {
    let (messages, initial_pending) = {
        let g = cfg_holder.read().await;
        let recorder = Arc::clone(&process_handles.tool_outcome_recorder);
        let fast = crate::runtime::workspace_session::repl_bootstrap_messages_fast(
            &g,
            agent_role_owned.as_deref(),
            &recorder,
        );
        if !g.session_ui.repl_initial_workspace_messages_enabled {
            (fast, None)
        } else {
            let may_scan_workspace = (g.context_bootstrap_inject.project_profile_inject_enabled
                && g.context_bootstrap_inject.project_profile_inject_max_chars > 0)
                || (g
                    .context_bootstrap_inject
                    .project_dependency_brief_inject_enabled
                    && g.context_bootstrap_inject
                        .project_dependency_brief_inject_max_chars
                        > 0)
                || (g.context_bootstrap_inject.agent_memory_file_enabled
                    && !g
                        .context_bootstrap_inject
                        .agent_memory_file
                        .trim()
                        .is_empty());
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
            let wd_bg = work_dir.to_path_buf();
            let role_for_bg = agent_role_owned.clone();
            let handles_bg = Arc::clone(&process_handles);
            std::thread::spawn(move || {
                let built = crate::runtime::workspace_session::initial_workspace_messages(
                    &cfg_bg,
                    wd_bg.as_path(),
                    tui_load,
                    role_for_bg.as_deref(),
                    &handles_bg.tool_outcome_recorder,
                );
                let mut guard = slot_bg.lock().unwrap_or_else(|e| e.into_inner());
                *guard = Some(built);
            });
            (fast, Some(slot))
        }
    };

    let history_dir = PathBuf::from(run_root).join(".crabmate");
    std::fs::create_dir_all(&history_dir)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let history_file = history_dir.join("repl_history.txt");
    let repl_editor = Arc::new(StdMutex::new(
        ReplLineEditor::new(history_file.as_path())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
    ));
    Ok((messages, initial_pending, repl_editor))
}

enum ReplMainIterationCtl {
    BreakRepl,
    Continue,
}

struct ReplIterationCtx<'a> {
    cfg_holder: &'a SharedAgentConfig,
    config_path: Option<&'a str>,
    client: &'a reqwest::Client,
    tools: &'a [crate::types::Tool],
    messages: &'a mut Vec<Message>,
    work_dir: &'a mut PathBuf,
    style: &'a CliReplStyle,
    no_stream: bool,
    agent_role_owned: &'a mut Option<String>,
    slash_handles: &'a ReplSlashSharedHandles,
    api_key_holder: &'a Arc<StdMutex<String>>,
    cli_rt: &'a CliToolRuntime,
    initial_pending: Option<&'a Arc<StdMutex<Option<Vec<Message>>>>>,
    process_handles: Arc<ProcessHandles>,
}

async fn repl_iteration_reply_to_read_line(
    read_res: ReplReadLine,
    ctx: &mut ReplIterationCtx<'_>,
) -> Result<ReplMainIterationCtl, Box<dyn std::error::Error>> {
    match read_res {
        ReplReadLine::Eof => Ok(ReplMainIterationCtl::BreakRepl),
        ReplReadLine::Empty => Ok(ReplMainIterationCtl::Continue),
        ReplReadLine::Shell(opt_cmd) => {
            let wd = ctx.work_dir.clone();
            let sty = ctx.style.clone();
            match tokio::task::spawn_blocking(move || {
                repl_execute_shell(opt_cmd.as_deref(), wd.as_path(), &sty)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let _ = ctx.style.eprint_error(&e.to_string());
                }
                Err(e) => {
                    let _ = ctx.style.eprint_error(&e.to_string());
                }
            }
            Ok(ReplMainIterationCtl::Continue)
        }
        ReplReadLine::Chat(input) => {
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
            })
            .await?;
            Ok(ReplMainIterationCtl::Continue)
        }
    }
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
    let slash_handles = ReplSlashSharedHandles {
        api_key_holder: Arc::clone(&api_key_holder),
        process_handles: Arc::clone(&process_handles),
    };

    {
        let g = cfg_holder.read().await;
        if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            g.system_prompt_for_new_conversation(Some(r))
                .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
        }
        let repl_llm_bearer_key_ready = !api_key.trim().is_empty();
        style.print_banner(
            &g,
            work_dir.as_ref(),
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
    let (mut messages, initial_pending, repl_editor) = repl_prepare_messages_and_editor(
        cfg_holder,
        tui_load,
        &work_dir,
        &agent_role_owned,
        run_root.as_str(),
        Arc::clone(&process_handles),
    )
    .await?;

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
        };
        match repl_iteration_reply_to_read_line(read_res, &mut iter_ctx).await? {
            ReplMainIterationCtl::BreakRepl => break,
            ReplMainIterationCtl::Continue => {}
        }
    }

    style.print_farewell()?;
    Ok(())
}
