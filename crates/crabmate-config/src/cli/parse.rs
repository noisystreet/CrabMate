//! `parse_args` / `parse_args_from_argv` 与 `RootCli` → [`ParsedCliArgs`] 映射。

use super::definitions::{
    BenchmarkCliArgs, Commands, E2eCliArgs, ExtraCliCommand, GlobalOpts, McpSubCmd, ParsedCliArgs,
    PluginInitCli, PluginListCli, PluginSubCmd, PluginValidateCli, RootCli, SaveSessionCli,
    SseReplayCli, ToolReplayCli, ToolReplaySubCmd, WebBearerCli, WebBearerSubCmd, WorkflowFileCli,
    WorkflowSubCmd,
};
use super::legacy_argv::normalize_legacy_argv;
use clap::Parser;
use std::io;

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
    llm_context_tokens_cli: Option<u32>,
}

impl CliParseCtx {
    fn new(global: &GlobalOpts) -> Self {
        let llm_context_tokens_cli = global.llm_context_tokens.filter(|&n| n > 0);
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
            llm_context_tokens_cli,
        }
    }

    fn base_parsed(&self) -> ParsedCliArgs {
        ParsedCliArgs {
            config_path: self.config_path.clone(),
            llm_context_tokens_cli: self.llm_context_tokens_cli,
            serve_port: None,
            serve_desktop_ready_json: false,
            http_bind_host: resolve_http_bind_host(None),
            workspace_cli: self.workspace_cli.clone(),
            no_tools: self.no_tools,
            with_web: false,
            dry_run: false,
            log_file: self.log_file.clone(),
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            web_bearer: None,
            tool_replay: None,
            sse_replay: None,
            plugin_init: None,
            plugin_validate: None,
            plugin_list: None,
            workflow_validate: None,
            workflow_compile: None,
            workflow_run: None,
            e2e: None,
        }
    }
}

/// 解析命令行：须显式子命令（**`serve` / `bench` / `config` / `doctor` / …**）；同进程 **`chat|repl|tui` 入口已移除**。
///
/// 非法 CLI：打印 clap 说明后以 **非零** 码退出进程（与历史 `parse_from` 行为一致）；**不会**向调用方返回 `Err`。
pub fn parse_args() -> io::Result<ParsedCliArgs> {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized).unwrap_or_else(|e| e.exit());
    Ok(build_parsed_cli_args(root))
}

/// 使用给定 **`argv`**（首元素为程序名）解析 CLI，供契约/集成测试；生产请用 [`parse_args`]。
///
/// - **`stdin_fixture`**：保留参数以兼容旧测试签名（同进程 `chat --stdin` 已移除，忽略）。
/// - 非法参数：返回 [`io::Error`]（**不**退出进程），便于断言。
pub fn parse_args_from_argv(
    raw: Vec<String>,
    _stdin_fixture: Option<String>,
) -> io::Result<ParsedCliArgs> {
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    Ok(build_parsed_cli_args(root))
}

fn build_parsed_cli_args(root: RootCli) -> ParsedCliArgs {
    let ctx = CliParseCtx::new(&root.global);
    let mut b = ctx.base_parsed();

    match root.command {
        Commands::Serve(s) => {
            b.serve_port = s.port.or(s.port_positional).or(Some(8080));
            b.serve_desktop_ready_json = s.desktop_ready_json;
            b.http_bind_host = resolve_http_bind_host(s.host);
            b.with_web = s.with_web;
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
        Commands::Config(c) => {
            b.dry_run = true;
            b.with_web = c.with_web;
        }
        Commands::Doctor => {
            b.extra_cli = ExtraCliCommand::Doctor;
        }
        Commands::WebBearer(w) => {
            b.web_bearer = Some(match w.sub {
                WebBearerSubCmd::Status => WebBearerCli::Status,
                WebBearerSubCmd::Set(s) => WebBearerCli::Set {
                    token: s.token,
                    stdin: s.stdin,
                    from_env: s.from_env,
                },
                WebBearerSubCmd::Clear => WebBearerCli::Clear,
            });
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
                projection: e.projection,
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
                judge: e.judge,
            });
        }
    }

    b
}
