//! `parse_args` / `parse_args_from_argv` 与 `RootCli` → [`ParsedCliArgs`] 映射。

use super::definitions::{
    BenchmarkCliArgs, ChatCliArgs, Commands, ExtraCliCommand, GlobalOpts, McpSubCmd, ParsedCliArgs,
    RootCli, SaveSessionCli, ToolReplayCli, ToolReplaySubCmd,
};
use super::legacy_argv::normalize_legacy_argv;
use clap::Parser;
use std::io::{self, Read};

/// 从标准输入读取全部内容（直到 EOF）
fn read_stdin_to_string() -> io::Result<String> {
    let mut s = String::new();
    io::stdin().read_to_string(&mut s)?;
    Ok(s)
}

fn parse_output_mode(raw: Option<String>) -> Option<String> {
    raw.as_ref().and_then(|m| {
        let m = m.to_ascii_lowercase();
        if m == "json" || m == "plain" {
            Some(m)
        } else {
            None
        }
    })
}

/// 解析命令行：支持 **`serve` / `repl` / `chat` / `bench` / `config` / `doctor` / `models` / `probe` / `mcp` / `save-session`**（兼容别名 **`export-session`**）、**`tool-replay`** 子命令，**`help`**（同 `--help` 或 `help <子命令>`），并兼容未写子命令时的历史平铺 flag（`--serve`、`--query` 等）。
///
/// `chat --stdin` 时若读取标准输入失败则返回 [`io::Error`]。
///
/// 非法 CLI：打印 clap 说明后以 **非零** 码退出进程（与历史 `parse_from` 行为一致）；**不会**向调用方返回 `Err`。
pub fn parse_args() -> io::Result<ParsedCliArgs> {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized).unwrap_or_else(|e| e.exit());
    build_parsed_cli_args(root, None)
}

/// 使用给定 **`argv`**（首元素为程序名）解析 CLI，供契约/集成测试；生产请用 [`parse_args`]。
///
/// - **`stdin_fixture`**：当参数含 `chat --stdin` 时，使用该字符串代替读取真实 stdin（避免测试挂起）。
/// - 非法参数：返回 [`io::Error`]（**不**退出进程），便于断言。
pub fn parse_args_from_argv(
    raw: Vec<String>,
    stdin_fixture: Option<String>,
) -> io::Result<ParsedCliArgs> {
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    build_parsed_cli_args(root, stdin_fixture)
}

fn build_parsed_cli_args(
    root: RootCli,
    stdin_fixture: Option<String>,
) -> io::Result<ParsedCliArgs> {
    let GlobalOpts {
        config,
        workspace,
        no_tools,
        log,
        agent_role,
    } = root.global;
    let agent_role_cli = agent_role
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let log_path = log
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let http_bind_host = |host_opt: Option<String>| {
        host_opt
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("AGENT_HTTP_HOST")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "127.0.0.1".to_string())
    };

    Ok(match root.command {
        None => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Serve(s)) => {
            let port = s.port.or(s.port_positional).or(Some(8080));
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: port,
                http_bind_host: http_bind_host(s.host),
                workspace_cli: workspace,
                no_tools,
                no_web: s.no_web,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: None,
            }
        }
        Some(Commands::Repl(r)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: r.no_stream,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Chat(c)) => {
            let inline_user_text = if c.user_prompt_file.is_some() {
                None
            } else if c.stdin {
                match stdin_fixture.as_ref() {
                    Some(s) => Some(s.clone()),
                    None => Some(read_stdin_to_string()?),
                }
            } else {
                c.query.clone()
            };
            let chat_output = parse_output_mode(c.output);
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs {
                    inline_user_text,
                    user_prompt_file: c.user_prompt_file,
                    system_prompt_file: c.system_prompt_file,
                    messages_json_file: c.messages_json_file,
                    message_file: c.message_file,
                    output: chat_output,
                    no_stream: c.no_stream,
                    yes_run_command: c.yes,
                    approve_commands: c.approve_commands,
                },
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools,
                no_web: false,
                dry_run: false,
                no_stream: c.no_stream,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: None,
            }
        }
        Some(Commands::Bench(b)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs {
                benchmark: b.benchmark,
                batch: b.batch,
                batch_output: b.batch_output,
                task_timeout: b.task_timeout,
                max_tool_rounds: b.max_tool_rounds,
                resume: b.resume,
                system_prompt_file: b.bench_system_prompt,
            },
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        // `config` 子命令恒走配置检查并退出，与是否写 `--dry-run` 无关（`--dry-run` 保留为显式别名）。
        Some(Commands::Config(_c)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: true,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Doctor) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Doctor,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Models) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Models,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Probe) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Probe,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::SaveSession(e)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: Some(SaveSessionCli {
                format: e.format,
                session_file: e.session_file,
            }),
            tool_replay: None,
        },
        Some(Commands::ToolReplay(tr)) => {
            let tr_cli = match tr.sub {
                ToolReplaySubCmd::Export(e) => ToolReplayCli::Export {
                    session_file: e.session_file,
                    output: e.output,
                    note: e.note,
                },
                ToolReplaySubCmd::Run(r) => ToolReplayCli::Run {
                    fixture: r.fixture,
                    compare_recorded: r.compare_recorded,
                },
            };
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools,
                no_web: false,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: Some(tr_cli),
            }
        }
        Some(Commands::Mcp(m)) => {
            let (extra_cli, no_tools_mcp) = match m.sub {
                McpSubCmd::List(l) => (ExtraCliCommand::McpList { probe: l.probe }, no_tools),
                McpSubCmd::Serve(s) => (
                    ExtraCliCommand::McpServe {
                        no_tools: s.no_tools,
                    },
                    no_tools,
                ),
            };
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools: no_tools_mcp,
                no_web: false,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli,
                save_session: None,
                tool_replay: None,
            }
        }
    })
}
