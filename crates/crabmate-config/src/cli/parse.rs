//! `parse_args` / `parse_args_from_argv` 与 `RootCli` → [`ParsedCliArgs`] 映射。

use super::definitions::{
    BenchmarkCliArgs, ChatCliArgs, Commands, E2eCliArgs, ExtraCliCommand, GlobalOpts, McpSubCmd,
    ParsedCliArgs, PluginInitCli, PluginListCli, PluginSubCmd, PluginValidateCli, RootCli,
    SaveSessionCli, SseReplayCli, ToolReplayCli, ToolReplaySubCmd, WorkflowFileCli, WorkflowSubCmd,
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

fn resolve_http_bind_host(host_opt: Option<String>) -> String {
    host_opt
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CM_HTTP_HOST")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// 全局选项解析一次后供各子命令分支复用（降低 `build_parsed_cli_args` 的 `nloc`）。
struct CliParseCtx {
    config_path: Option<String>,
    workspace_cli: Option<String>,
    no_tools: bool,
    log_file: Option<String>,
    agent_role_cli: Option<String>,
    llm_context_tokens_cli: Option<u32>,
}

impl CliParseCtx {
    fn new(global: &GlobalOpts) -> Self {
        let llm_context_tokens_cli = global.llm_context_tokens.filter(|&n| n > 0);
        let agent_role_cli = global
            .agent_role
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let log_file = global
            .log
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self {
            config_path: global.config.clone(),
            workspace_cli: global.workspace.clone(),
            no_tools: global.no_tools,
            log_file,
            agent_role_cli,
            llm_context_tokens_cli,
        }
    }

    fn base_parsed(&self) -> ParsedCliArgs {
        ParsedCliArgs {
            config_path: self.config_path.clone(),
            agent_role_cli: self.agent_role_cli.clone(),
            llm_context_tokens_cli: self.llm_context_tokens_cli,
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            serve_desktop_ready_json: false,
            http_bind_host: resolve_http_bind_host(None),
            workspace_cli: self.workspace_cli.clone(),
            no_tools: self.no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: self.log_file.clone(),
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
            sse_replay: None,
            plugin_init: None,
            plugin_validate: None,
            plugin_list: None,
            workflow_validate: None,
            workflow_compile: None,
            workflow_run: None,
            tui: false,
            e2e: None,
        }
    }
}

/// 解析命令行：支持 **`serve` / `repl` / `tui` / `chat` / `bench` / `config` / `doctor` / `models` / `probe` / `mcp` / `save-session`**（兼容别名 **`export-session`**）、**`tool-replay`** 子命令，**`help`**（同 `--help` 或 `help <子命令>`），并兼容未写子命令时的历史平铺 flag（`--serve`、`--query` 等）。
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
    let ctx = CliParseCtx::new(&root.global);
    let cmd = match root.command {
        None => return Ok(ctx.base_parsed()),
        Some(c) => c,
    };
    let mut b = ctx.base_parsed();

    match cmd {
        Commands::Serve(s) => {
            b.serve_port = s.port.or(s.port_positional).or(Some(8080));
            b.serve_desktop_ready_json = s.desktop_ready_json;
            b.http_bind_host = resolve_http_bind_host(s.host);
            b.no_web = s.no_web;
        }
        Commands::Repl(r) => {
            b.no_stream = r.no_stream;
        }
        Commands::Tui => {
            b.tui = true;
        }
        Commands::Chat(c) => {
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
            b.chat_cli = ChatCliArgs {
                inline_user_text,
                user_prompt_file: c.user_prompt_file,
                system_prompt_file: c.system_prompt_file,
                messages_json_file: c.messages_json_file,
                message_file: c.message_file,
                output: chat_output,
                no_stream: c.no_stream,
                yes_run_command: c.yes,
                approve_commands: c.approve_commands,
            };
            b.no_stream = c.no_stream;
        }
        Commands::Bench(be) => {
            b.bench_args = BenchmarkCliArgs {
                benchmark: be.benchmark,
                batch: be.batch,
                batch_output: be.batch_output,
                task_timeout: be.task_timeout,
                max_tool_rounds: be.max_tool_rounds,
                resume: be.resume,
                system_prompt_file: be.bench_system_prompt,
            };
        }
        Commands::Config(_) => {
            b.dry_run = true;
        }
        Commands::Doctor => {
            b.extra_cli = ExtraCliCommand::Doctor;
        }
        Commands::Models => {
            b.extra_cli = ExtraCliCommand::Models;
        }
        Commands::Probe => {
            b.extra_cli = ExtraCliCommand::Probe;
        }
        Commands::SaveSession(e) => {
            b.save_session = Some(SaveSessionCli {
                format: e.format,
                session_file: e.session_file,
            });
        }
        Commands::ToolReplay(tr) => {
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
            b.tool_replay = Some(tr_cli);
        }
        Commands::SseReplay(sr) => {
            b.sse_replay = Some(SseReplayCli {
                file: sr.file,
                format: sr.format,
                job_id: sr.job_id,
            });
        }
        Commands::Mcp(m) => {
            let (extra_cli, no_tools_mcp) = match m.sub {
                McpSubCmd::List(l) => (ExtraCliCommand::McpList { probe: l.probe }, ctx.no_tools),
                McpSubCmd::Serve(s) => (
                    ExtraCliCommand::McpServe {
                        no_tools: s.no_tools,
                        port: s.port,
                    },
                    ctx.no_tools,
                ),
            };
            b.extra_cli = extra_cli;
            b.no_tools = no_tools_mcp;
        }
        Commands::Plugin(p) => {
            let (plugin_init, plugin_validate, plugin_list) = match p.sub {
                PluginSubCmd::Init(i) => (
                    Some(PluginInitCli {
                        name: i.name,
                        description: i.description,
                        command: i.command,
                        args: i.args,
                        pass_args_json: i.pass_args_json,
                        output: i.output,
                    }),
                    None,
                    None,
                ),
                PluginSubCmd::List(l) => (
                    None,
                    None,
                    Some(PluginListCli {
                        file: l.file,
                        json: l.json,
                        jsonl: l.jsonl,
                    }),
                ),
                PluginSubCmd::Validate(v) => (
                    None,
                    Some(PluginValidateCli {
                        file: v.file,
                        json: v.json,
                        jsonl: v.jsonl,
                    }),
                    None,
                ),
            };
            b.plugin_init = plugin_init;
            b.plugin_validate = plugin_validate;
            b.plugin_list = plugin_list;
        }
        Commands::Workflow(w) => match w.sub {
            WorkflowSubCmd::Validate(v) => {
                b.workflow_validate = Some(WorkflowFileCli {
                    file: v.file,
                    json: v.json,
                });
            }
            WorkflowSubCmd::Compile(c) => {
                b.workflow_compile = Some(WorkflowFileCli {
                    file: c.file,
                    json: c.json,
                });
            }
            WorkflowSubCmd::Run(r) => {
                b.workflow_run = Some(WorkflowFileCli {
                    file: r.file,
                    json: r.json,
                });
            }
        },
        Commands::E2e(e) => {
            b.e2e = Some(E2eCliArgs {
                mode: e.mode,
                output_dir: e.output_dir,
                recordings_dir: e.recordings_dir,
                scenarios_file: e.scenarios_file,
            });
        }
    }

    Ok(b)
}
