use crate::config::AgentConfig;
use crate::redact;
use crate::tool_registry::CliToolRuntime;
use crate::types::{Message, messages_chat_seed};
use crate::{LlmSeedOverride, RunAgentTurnParams, run_agent_turn};
use crossterm::{
    ExecutableCommand,
    cursor::MoveToColumn,
    terminal::{Clear, ClearType},
};
use log::debug;
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;
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

/// 单次提问模式（--query / --stdin），执行一轮对话后退出
#[allow(clippy::too_many_arguments)]
pub async fn run_single_shot(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    output_mode: &Option<String>,
    no_stream: bool,
    question: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let q = question.trim();
    if q.is_empty() {
        return Err("错误：--query 或 --stdin 内容为空".into());
    }
    let mut messages = messages_chat_seed(&cfg.system_prompt, q);
    debug!(
        target: "crabmate::print",
        "单次提问模式 seed 消息已构造 user_preview={}",
        redact::preview_chars(q, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let work_dir = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    let cli_rt = CliToolRuntime {
        persistent_allowlist_shared: Arc::new(Mutex::new(HashSet::new())),
    };
    run_agent_turn_for_cli(
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
    .map_err(|e| -> Box<dyn std::error::Error> { e })?;
    if let Some(mode) = output_mode.as_deref()
        && mode == "json"
    {
        let reply = messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        let obj = serde_json::json!({
            "reply": reply,
            "model": cfg.model,
        });
        println!("{}", obj);
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
    let cli_rt = CliToolRuntime {
        persistent_allowlist_shared: Arc::new(Mutex::new(HashSet::new())),
    };

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
