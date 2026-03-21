use crate::config::AgentConfig;
use crate::types::Message;
use crate::run_agent_turn;
use crossterm::{cursor::MoveToColumn, terminal::{Clear, ClearType}, ExecutableCommand};
use std::io::{self, Write};

/// 单次提问模式（--query / --stdin），执行一轮对话后退出
#[allow(clippy::too_many_arguments)]
pub async fn run_single_shot(
    cfg: &AgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    output_mode: &Option<String>,
    no_stream: bool,
    question: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut messages: Vec<Message> = vec![Message {
        role: "system".to_string(),
        content: Some(cfg.system_prompt.clone()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }];

    let q = question.trim();
    if q.is_empty() {
        eprintln!("错误：--query 或 --stdin 内容为空");
        std::process::exit(1);
    }
    messages.push(Message {
        role: "user".to_string(),
        content: Some(q.to_string()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    });
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
    )
    .await
    {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    if let Some(mode) = output_mode.as_deref()
        && mode == "json" {
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
    cfg: &AgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut messages: Vec<Message> = vec![Message {
        role: "system".to_string(),
        content: Some(cfg.system_prompt.clone()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }];

    println!(
        "=== DeepSeek Agent Demo ===\n当前模型: {}\n输入内容与 Agent 对话，输入 quit/exit 或 Ctrl+D 退出。\n",
        cfg.model
    );

    loop {
        // 清理提示符所在行，避免被上一次输出残留影响
        let mut stdout = io::stdout();
        let _ = stdout.execute(MoveToColumn(0));
        let _ = stdout.execute(Clear(ClearType::CurrentLine));
        write!(stdout, "你: ")?;
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

        messages.push(Message {
            role: "user".to_string(),
            content: Some(input.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });

        let work_dir_str = workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&cfg.run_command_working_dir)
            .to_string();
        if let Err(e) = run_agent_turn(
            client,
            api_key,
            cfg,
            tools,
            &mut messages,
            None,
            std::path::Path::new(&work_dir_str),
            true,
            !no_stream,
            no_stream,
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

