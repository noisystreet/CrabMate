use crate::config::AgentConfig;
use crate::config::cli::ChatCliArgs;
use crate::redact;
use crate::runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
    classify_model_error_message,
};
use crate::tool_registry::{CliCommandTurnStats, CliToolRuntime};
use crate::types::{Message, messages_chat_seed, normalize_messages_for_openai_compatible_request};
use crate::{LlmSeedOverride, RunAgentTurnParams, run_agent_turn};
use crossterm::{
    ExecutableCommand,
    cursor::MoveToColumn,
    terminal::{Clear, ClearType},
};
use log::debug;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
enum ReplBuiltIn<'a> {
    Clear,
    Model,
    WorkspaceShow,
    WorkspaceSet(&'a str),
    Tools,
    Help,
    Unknown(&'a str),
    BareSlash,
}

/// 解析 REPL 行首 `/` 内建命令；非内建前缀返回 `None`。
fn classify_repl_slash_command(input: &str) -> Option<ReplBuiltIn<'_>> {
    let s = input.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = s[1..].trim();
    if rest.is_empty() {
        return Some(ReplBuiltIn::BareSlash);
    }
    let head = rest.split_whitespace().next().unwrap_or("");
    let cmd = head.to_ascii_lowercase();
    let arg = rest[head.len()..].trim();
    Some(match cmd.as_str() {
        "clear" => ReplBuiltIn::Clear,
        "model" => ReplBuiltIn::Model,
        "workspace" | "cd" => {
            if arg.is_empty() {
                ReplBuiltIn::WorkspaceShow
            } else {
                ReplBuiltIn::WorkspaceSet(arg)
            }
        }
        "tools" => ReplBuiltIn::Tools,
        "help" | "?" => ReplBuiltIn::Help,
        _ => ReplBuiltIn::Unknown(head),
    })
}

/// REPL 中以 `/` 开头的内建命令；返回 `true` 表示已处理（不调用模型）。
fn try_handle_repl_slash_command(
    input: &str,
    cfg: &AgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &mut PathBuf,
) -> bool {
    let Some(builtin) = classify_repl_slash_command(input) else {
        return false;
    };
    match builtin {
        ReplBuiltIn::BareSlash => {
            println!("输入 /help 查看内建命令；若以 / 开头的文字要发给模型，请避免仅输入一个 /。");
        }
        ReplBuiltIn::Unknown(head) => {
            eprintln!("未知命令 /{}。输入 /help 查看列表。", head);
        }
        ReplBuiltIn::Clear => {
            *messages = vec![Message::system_only(cfg.system_prompt.clone())];
            println!("已清空对话（保留当前 system 提示词），共 1 条消息。");
        }
        ReplBuiltIn::Model => {
            println!("model: {}", cfg.model);
            println!("api_base: {}", cfg.api_base);
            println!(
                "temperature: {}（配置文件；Web chat 可单条覆盖）",
                cfg.temperature
            );
            if let Some(seed) = cfg.llm_seed {
                println!("llm_seed: {seed}");
            } else {
                println!("llm_seed: （未设置，请求不带 seed）");
            }
        }
        ReplBuiltIn::WorkspaceShow => match work_dir.canonicalize() {
            Ok(p) => println!("当前工作区: {}", p.display()),
            Err(_) => println!("当前工作区: {}", work_dir.display()),
        },
        ReplBuiltIn::WorkspaceSet(arg) => {
            let candidate = PathBuf::from(arg);
            let resolved = match candidate.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("无法解析路径 {:?}: {}", arg, e);
                    return true;
                }
            };
            if !resolved.is_dir() {
                eprintln!("不是目录: {}", resolved.display());
                return true;
            }
            *work_dir = resolved;
            println!("工作区已切换为: {}", work_dir.display());
        }
        ReplBuiltIn::Tools => {
            if tools.is_empty() {
                println!("当前未加载工具（可能使用了 --no-tools）。");
            } else {
                println!("当前 {} 个工具:", tools.len());
                for t in tools {
                    println!("  - {}", t.function.name);
                }
            }
        }
        ReplBuiltIn::Help => {
            println!("内建命令（不发给模型）：");
            println!("  /clear     清空对话历史，仅保留当前 system 提示词");
            println!("  /model     显示 model、api_base、temperature、llm_seed");
            println!("  /workspace 显示当前工作区路径");
            println!("  /workspace <路径>  切换工作区（须为已存在目录，别名 /cd）");
            println!("  /tools     列出当前加载的工具名");
            println!("  /help      本说明");
            println!("非白名单 run_command：终端会提示 y（一次）/ a（本会话永久允许该命令名）");
            println!("退出：quit / exit 或 Ctrl+D");
        }
    }
    true
}

fn cli_effective_work_dir(workspace_cli: &Option<String>, default: &str) -> PathBuf {
    PathBuf::from(
        workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default),
    )
}

/// CLI（无 SSE、`workspace_is_set` 恒为真）下调用 [`run_agent_turn`] 的固定参数封装。
#[allow(clippy::too_many_arguments)] // CLI 与可选 cli_tool_ctx 并列，聚合为结构体收益有限
async fn run_agent_turn_for_cli(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &Arc<AgentConfig>,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &std::path::Path,
    no_stream: bool,
    cli_tool_ctx: Option<&CliToolRuntime>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_agent_turn(RunAgentTurnParams {
        client,
        api_key,
        cfg,
        tools,
        messages,
        out: None,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        render_to_terminal: true,
        no_stream,
        cancel: None,
        per_flight: None,
        web_tool_ctx: None,
        cli_tool_ctx,
        plain_terminal_stream: true,
        llm_backend: None,
        temperature_override: None,
        seed_override: LlmSeedOverride::default(),
    })
    .await
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
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = chat.system_prompt_file.as_deref() {
        let t = std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --system-prompt-file {p}: {e}"),
            )
        })?;
        return Ok(t);
    }
    Ok(cfg.system_prompt.clone())
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
        .and_then(|m| m.content.clone())
        .unwrap_or_default();
    let mut obj = serde_json::json!({
        "reply": reply,
        "model": cfg.model,
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

#[allow(clippy::too_many_arguments)]
async fn run_one_cli_turn(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &Arc<AgentConfig>,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &Path,
    no_stream: bool,
    cli_rt: &CliToolRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    run_agent_turn_for_cli(
        client,
        api_key,
        cfg,
        tools,
        messages,
        work_dir,
        no_stream,
        Some(cli_rt),
    )
    .await
    .map_err(map_turn_err)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_chat_batch_jsonl(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    work_dir: &Path,
    no_stream: bool,
    cli_rt: &CliToolRuntime,
    json_out: bool,
    path: &str,
    chat: &ChatCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path).map_err(|e| {
        CliExitError::new(EXIT_GENERAL, format!("无法打开 --message-file {path}: {e}"))
    })?;
    let reader = std::io::BufReader::new(file);
    let system_seed = resolve_system_prompt_for_chat(cfg, chat)?;
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
                messages = messages_chat_seed(&system_seed, u);
            } else {
                messages.push(Message::user_only(u.to_string()));
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

        run_one_cli_turn(
            client,
            api_key,
            cfg,
            tools,
            &mut messages,
            work_dir,
            no_stream,
            cli_rt,
        )
        .await?;
        ensure_all_run_commands_not_denied(cli_rt)?;
        if json_out {
            print_json_reply_line(cfg, &messages, Some(line_no));
        }
    }
    Ok(())
}

/// `chat` 子命令：单轮、整表 JSON、或 `--message-file` 多轮批跑。
pub async fn run_chat_invocation(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    chat: &ChatCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let work_dir = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    let cli_rt = build_cli_runtime(chat);
    let json_out = chat.output.as_deref().is_some_and(|m| m == "json");

    if let Some(batch_path) = chat.message_file.as_deref() {
        return run_chat_batch_jsonl(
            cfg,
            client,
            api_key,
            tools,
            work_dir.as_path(),
            chat.no_stream,
            &cli_rt,
            json_out,
            batch_path,
            chat,
        )
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
        run_one_cli_turn(
            client,
            api_key,
            cfg,
            tools,
            &mut messages,
            work_dir.as_path(),
            chat.no_stream,
            &cli_rt,
        )
        .await?;
        ensure_all_run_commands_not_denied(&cli_rt)?;
        if json_out {
            print_json_reply_line(cfg, &messages, None);
        }
        return Ok(());
    }

    let system = resolve_system_prompt_for_chat(cfg, chat)?;
    let user = resolve_user_body(chat)?;
    let mut messages = messages_chat_seed(&system, &user);
    debug!(
        target: "crabmate::print",
        "chat 首轮已构造 system_len={} user_preview={}",
        system.len(),
        redact::preview_chars(&user, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    run_one_cli_turn(
        client,
        api_key,
        cfg,
        tools,
        &mut messages,
        work_dir.as_path(),
        chat.no_stream,
        &cli_rt,
    )
    .await?;
    ensure_all_run_commands_not_denied(&cli_rt)?;
    if json_out {
        print_json_reply_line(cfg, &messages, None);
    }
    Ok(())
}

/// 交互式 REPL 模式
pub async fn run_repl(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut work_dir = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    let mut messages = crate::runtime::workspace_session::initial_workspace_messages(
        cfg.as_ref(),
        work_dir.as_path(),
        cfg.tui_load_session_on_start,
    );
    let cli_rt = CliToolRuntime::new_interactive_default();

    println!(
        "当前模型: {}\n输入内容与 Agent 对话；内建命令见 /help。quit/exit 或 Ctrl+D 退出。\n非白名单 run_command 将在终端询问确认（y 一次 / a 本会话永久允许该命令名）。\n",
        cfg.model
    );

    loop {
        // 清理提示符所在行，避免被上一次输出残留影响
        let mut stdout = io::stdout();
        let _ = stdout.execute(MoveToColumn(0));
        let _ = stdout.execute(Clear(ClearType::CurrentLine));
        crate::runtime::terminal_labels::write_user_message_prefix(&mut stdout)?;
        stdout.flush()?;
        let (n, input) = tokio::task::spawn_blocking(|| {
            let mut input = String::new();
            let n = io::stdin().read_line(&mut input)?;
            Ok::<_, io::Error>((n, input))
        })
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        if n == 0 {
            break; // Ctrl+D (EOF)
        }
        let input = input.trim();
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            break;
        }
        if input.is_empty() {
            continue;
        }

        if try_handle_repl_slash_command(input, cfg.as_ref(), tools, &mut messages, &mut work_dir) {
            continue;
        }

        messages.push(Message::user_only(input.to_string()));
        debug!(
            target: "crabmate::print",
            "REPL 用户输入已入队 history_len={} input_preview={}",
            messages.len(),
            redact::preview_chars(input, redact::MESSAGE_LOG_PREVIEW_CHARS)
        );

        if let Err(e) = run_agent_turn_for_cli(
            client,
            api_key,
            cfg,
            tools,
            &mut messages,
            work_dir.as_path(),
            no_stream,
            Some(&cli_rt),
        )
        .await
        {
            eprintln!("{}", e);
            break;
        }
    }

    println!("再见。");
    Ok(())
}

#[cfg(test)]
mod repl_slash_tests {
    use super::{ReplBuiltIn, classify_repl_slash_command};

    #[test]
    fn not_slash_is_none() {
        assert!(classify_repl_slash_command("hello").is_none());
    }

    #[test]
    fn bare_slash() {
        assert_eq!(
            classify_repl_slash_command("  /  "),
            Some(ReplBuiltIn::BareSlash)
        );
    }

    #[test]
    fn clear_model_tools_help() {
        assert_eq!(
            classify_repl_slash_command("/CLEAR"),
            Some(ReplBuiltIn::Clear)
        );
        assert_eq!(
            classify_repl_slash_command("/model"),
            Some(ReplBuiltIn::Model)
        );
        assert_eq!(
            classify_repl_slash_command("/tools"),
            Some(ReplBuiltIn::Tools)
        );
        assert_eq!(
            classify_repl_slash_command("/help"),
            Some(ReplBuiltIn::Help)
        );
        assert_eq!(classify_repl_slash_command("/?"), Some(ReplBuiltIn::Help));
    }

    #[test]
    fn workspace_and_cd() {
        assert_eq!(
            classify_repl_slash_command("/workspace"),
            Some(ReplBuiltIn::WorkspaceShow)
        );
        assert_eq!(
            classify_repl_slash_command("/workspace /tmp"),
            Some(ReplBuiltIn::WorkspaceSet("/tmp"))
        );
        assert_eq!(
            classify_repl_slash_command("  /cd  ./foo  "),
            Some(ReplBuiltIn::WorkspaceSet("./foo"))
        );
    }

    #[test]
    fn unknown() {
        assert_eq!(
            classify_repl_slash_command("/nope"),
            Some(ReplBuiltIn::Unknown("nope"))
        );
    }
}
