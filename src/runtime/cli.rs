use crate::config::AgentConfig;
use crate::redact;
use crate::run_agent_turn;
use crate::types::{Message, messages_chat_seed};
use crossterm::{
    ExecutableCommand,
    cursor::MoveToColumn,
    terminal::{Clear, ClearType},
};
use log::debug;
use std::io::{self, Write};
use std::sync::Arc;

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
        eprintln!("错误：--query 或 --stdin 内容为空");
        std::process::exit(1);
    }
    let mut messages = messages_chat_seed(&cfg.system_prompt, q);
    debug!(
        target: "crabmate::print",
        "单次提问模式 seed 消息已构造 user_preview={}",
        redact::preview_chars(q, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let work_dir_str = workspace_cli
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&cfg.run_command_working_dir)
        .to_string();
    let work_dir = std::path::Path::new(&work_dir_str);
    if let Err(e) = run_agent_turn(
        client,
        api_key,
        cfg,
        tools,
        &mut messages,
        None,
        work_dir,
        true,
        !no_stream,
        no_stream,
        None,
        None,
        None,
    )
    .await
    {
        eprintln!("{}", e);
        std::process::exit(1);
    }
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
    let work_dir_str = workspace_cli
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&cfg.run_command_working_dir)
        .to_string();
    let work_dir = std::path::Path::new(&work_dir_str);
    let mut messages = crate::runtime::workspace_session::initial_workspace_messages(
        cfg.as_ref(),
        work_dir,
        cfg.tui_load_session_on_start,
    );

    println!(
        "=== DeepSeek Agent Demo ===\n当前模型: {}\n输入内容与 Agent 对话，输入 quit/exit 或 Ctrl+D 退出。\n",
        cfg.model
    );

    loop {
        // 清理提示符所在行，避免被上一次输出残留影响
        let mut stdout = io::stdout();
        let _ = stdout.execute(MoveToColumn(0));
        let _ = stdout.execute(Clear(ClearType::CurrentLine));
        crate::runtime::terminal_labels::write_user_message_prefix(&mut stdout)?;
        stdout.flush()?;
        let mut input = String::new();
        let n = io::stdin().read_line(&mut input)?;
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

        messages.push(Message::user_only(input.to_string()));
        debug!(
            target: "crabmate::print",
            "REPL 用户输入已入队 history_len={} input_preview={}",
            messages.len(),
            redact::preview_chars(input, redact::MESSAGE_LOG_PREVIEW_CHARS)
        );

        if let Err(e) = run_agent_turn(
            client,
            api_key,
            cfg,
            tools,
            &mut messages,
            None,
            work_dir,
            true,
            !no_stream,
            no_stream,
            None,
            None,
            None,
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
