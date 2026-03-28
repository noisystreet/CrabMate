use crate::config::AgentConfig;
use crate::config::cli::ChatCliArgs;
use crate::redact;
use crate::runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
    classify_model_error_message,
};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::tool_registry::{CliCommandTurnStats, CliToolRuntime};
use crate::types::{Message, messages_chat_seed, normalize_messages_for_openai_compatible_request};
use crate::{LlmSeedOverride, RunAgentTurnParams, run_agent_turn};
use crossterm::{
    ExecutableCommand,
    cursor::MoveToColumn,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use log::debug;
use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

/// 长期记忆库打开失败时，仅向 stderr 打印**一次**用户可见说明（避免每轮 REPL/chat 重复刷屏）。
static CLI_LTM_OPEN_FAILURE_NOTIFIED: AtomicBool = AtomicBool::new(false);

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

/// 行首 `$`：`Some(None)` 表示仅 `$`（应打印用法）；`Some(Some(cmd))` 为待执行的 shell 一行；`None` 表示非 `$` 行。
fn parse_repl_dollar_shell_line(input: &str) -> Option<Option<&str>> {
    let t = input.trim_start();
    if !t.starts_with('$') {
        return None;
    }
    let rest = t[1..].trim();
    if rest.is_empty() {
        Some(None)
    } else {
        Some(Some(rest))
    }
}

const REPL_SHELL_USAGE: &str = "bash#: <命令>  在当前工作区执行一行 shell（不发给模型；无交互 stdin）。等同本机 `sh -c` / `cmd /C`，不受模型 `run_command` 白名单约束，仅应在可信环境使用。交互终端：行首按 `$` 后提示变为 bash#:（`$` 不回显）；管道输入仍可用 `$ <命令>`。示例: ls  pwd  git status";

/// 执行 REPL 本地 shell 一行：`parsed` 为 `parse_repl_dollar_shell_line` 的 `Some(...)` 内层；`None` 表示仅 `$` 或空命令，打印用法。
fn repl_execute_shell(
    parsed: Option<&str>,
    work_dir: &Path,
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

/// REPL 单次读取结果（在 `spawn_blocking` 内完成）。
#[derive(Debug)]
enum ReplReadLine {
    /// stdin EOF（如 Ctrl+D）
    Eof,
    /// 空行（忽略，继续下一轮）
    Empty,
    /// 普通对话文本
    Chat(String),
    /// 本地 shell：`None` 为应打印用法；`Some` 为已去掉 `$` 的命令行
    Shell(Option<String>),
}

fn read_repl_line_blocking() -> io::Result<ReplReadLine> {
    let mut stdout = io::stdout();
    let _ = stdout.execute(MoveToColumn(0));
    let _ = stdout.execute(Clear(ClearType::CurrentLine));
    crate::runtime::terminal_labels::write_user_message_prefix(&mut stdout)?;
    stdout.flush()?;

    if io::stdin().is_terminal() && stdout.is_terminal() {
        read_repl_line_tty(stdout)
    } else {
        read_repl_line_piped()
    }
}

fn read_repl_line_piped() -> io::Result<ReplReadLine> {
    let mut input = String::new();
    let n = io::stdin().lock().read_line(&mut input)?;
    if n == 0 {
        return Ok(ReplReadLine::Eof);
    }
    let line = input.trim_end_matches(['\r', '\n']);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(ReplReadLine::Empty);
    }
    if let Some(inner) = parse_repl_dollar_shell_line(trimmed) {
        Ok(ReplReadLine::Shell(inner.map(str::to_string)))
    } else {
        Ok(ReplReadLine::Chat(trimmed.to_string()))
    }
}

fn redraw_repl_input_prompt(stdout: &mut io::Stdout, shell_mode: bool) -> io::Result<()> {
    queue!(stdout, MoveToColumn(0))?;
    stdout.execute(Clear(ClearType::CurrentLine))?;
    if shell_mode {
        crate::runtime::terminal_labels::write_repl_bash_prompt_prefix(stdout)?;
    } else {
        crate::runtime::terminal_labels::write_user_message_prefix(stdout)?;
    }
    stdout.flush()?;
    Ok(())
}

/// 保证离开 `read_repl_line_tty` 时（含 panic 栈展开）尽量恢复 cooked 模式，避免终端卡在 raw。
struct ReplRawModeGuard;

impl Drop for ReplRawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn read_repl_line_tty(mut stdout: io::Stdout) -> io::Result<ReplReadLine> {
    enable_raw_mode()?;
    let _raw_guard = ReplRawModeGuard;
    (|| -> io::Result<ReplReadLine> {
        let mut buf = String::new();
        let mut shell_prompt = false;
        loop {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    KeyCode::Enter => {
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        if shell_prompt {
                            if buf.trim().is_empty() {
                                return Ok(ReplReadLine::Shell(None));
                            }
                            return Ok(ReplReadLine::Shell(Some(buf)));
                        }
                        if buf.trim().is_empty() {
                            return Ok(ReplReadLine::Empty);
                        }
                        return Ok(ReplReadLine::Chat(buf));
                    }
                    KeyCode::Backspace => {
                        if buf.pop().is_some() {
                            write!(stdout, "\x08 \x08")?;
                            stdout.flush()?;
                        } else if shell_prompt {
                            shell_prompt = false;
                            redraw_repl_input_prompt(&mut stdout, false)?;
                        }
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        return Ok(ReplReadLine::Eof);
                    }
                    KeyCode::Char(c) => {
                        if c == '$' && !shell_prompt && buf.is_empty() {
                            shell_prompt = true;
                            redraw_repl_input_prompt(&mut stdout, true)?;
                            continue;
                        }
                        buf.push(c);
                        write!(stdout, "{c}")?;
                        stdout.flush()?;
                    }
                    _ => {}
                }
            }
        }
    })()
}

fn run_repl_shell_line_sync(cmd: &str, work_dir: &Path) -> io::Result<i32> {
    let status = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    } else {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    };
    Ok(status
        .code()
        .unwrap_or(if status.success() { 0 } else { -1 }))
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
    style: &CliReplStyle,
) -> bool {
    let Some(builtin) = classify_repl_slash_command(input) else {
        return false;
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
            *messages = vec![Message::system_only(cfg.system_prompt.clone())];
            let _ = style.print_success("已清空对话（保留当前 system 提示词），共 1 条消息。");
        }
        ReplBuiltIn::Model => {
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
        ReplBuiltIn::WorkspaceShow => match work_dir.canonicalize() {
            Ok(p) => {
                let _ = style.print_line(&format!("当前工作区: {}", p.display()));
            }
            Err(_) => {
                let _ = style.print_line(&format!("当前工作区: {}", work_dir.display()));
            }
        },
        ReplBuiltIn::WorkspaceSet(arg) => {
            let candidate = PathBuf::from(arg);
            let resolved = match candidate.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    let _ = style.eprint_error(&format!("无法解析路径 {arg:?}: {e}"));
                    return true;
                }
            };
            if !resolved.is_dir() {
                let _ = style.eprint_error(&format!("不是目录: {}", resolved.display()));
                return true;
            }
            *work_dir = resolved;
            let _ = style.print_success(&format!("工作区已切换为: {}", work_dir.display()));
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
    let (ltm, scope) = cli_long_term_memory_handles(cfg);
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
        long_term_memory: ltm,
        long_term_memory_scope_id: scope,
        read_file_turn_cache: None,
    })
    .await
}

fn cli_long_term_memory_handles(
    cfg: &Arc<AgentConfig>,
) -> (
    Option<std::sync::Arc<crate::long_term_memory::LongTermMemoryRuntime>>,
    Option<String>,
) {
    if !cfg.long_term_memory_enabled {
        return (None, None);
    }
    let path = cfg.long_term_memory_store_sqlite_path.trim();
    let p = if path.is_empty() {
        let base = std::path::Path::new(&cfg.run_command_working_dir).join(".crabmate");
        base.join("long_term_memory.db")
    } else {
        std::path::PathBuf::from(path)
    };
    match crate::long_term_memory::cli_runtime_lazy(&p) {
        Ok(r) => (Some(r), Some("cli".to_string())),
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "CLI 长期记忆库打开失败 path={} error={}",
                p.display(),
                e
            );
            if !CLI_LTM_OPEN_FAILURE_NOTIFIED.swap(true, Ordering::SeqCst) {
                let detail = e.to_string();
                let max = 240usize;
                let (head, tail) = if detail.chars().count() > max {
                    let head: String = detail.chars().take(max).collect();
                    (head, "…")
                } else {
                    (detail, "")
                };
                eprintln!(
                    "crabmate: 警告：配置中已启用长期记忆 (long_term_memory_enabled)，但本进程无法打开 SQLite；长期记忆在本进程中已禁用。\n\
                     路径: {}\n\
                     错误: {}{}\n\
                     请检查目录权限、磁盘空间或向量后端依赖（如 fastembed / ONNX）；若暂不需要可设 long_term_memory_enabled = false。详情见日志 (target=crabmate)。",
                    p.display(),
                    head,
                    tail
                );
            }
            (None, None)
        }
    }
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
    let style = CliReplStyle::new();

    style.print_banner(cfg.as_ref(), work_dir.as_path(), tools.len(), no_stream)?;

    loop {
        let read_res = tokio::task::spawn_blocking(read_repl_line_blocking)
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

                if try_handle_repl_slash_command(
                    input.as_str(),
                    cfg.as_ref(),
                    tools,
                    &mut messages,
                    &mut work_dir,
                    &style,
                ) {
                    continue;
                }

                messages.push(Message::user_only(input.to_string()));
                debug!(
                    target: "crabmate::print",
                    "REPL 用户输入已入队 history_len={} input_preview={}",
                    messages.len(),
                    redact::preview_chars(input.as_str(), redact::MESSAGE_LOG_PREVIEW_CHARS)
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

#[cfg(test)]
mod repl_dollar_tests {
    use super::{parse_repl_dollar_shell_line, run_repl_shell_line_sync};

    #[test]
    fn parse_not_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("hello"), None);
    }

    #[test]
    fn parse_bare_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("$"), Some(None));
    }

    #[test]
    fn parse_dollar_ls() {
        assert_eq!(parse_repl_dollar_shell_line("$ ls"), Some(Some("ls")));
    }

    #[test]
    fn parse_dollar_leading_space() {
        assert_eq!(
            parse_repl_dollar_shell_line("  $ echo x"),
            Some(Some("echo x"))
        );
    }

    #[test]
    fn shell_true_zero_exit() {
        let dir = std::env::temp_dir();
        let cmd = if cfg!(windows) { "exit /b 0" } else { "true" };
        let code = run_repl_shell_line_sync(cmd, &dir).unwrap();
        assert_eq!(code, 0);
    }
}
