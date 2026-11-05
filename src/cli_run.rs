//! `cargo run` / 库入口 [`crate::run`] 的 CLI 编排（从 `lib.rs` 拆出以降低单函数圈复杂度）。
//!
//! **S5（#1295）：一次 `match` 分派**。这里不再有 `EarlyCliDispatch` + 一串 `try_early_*`。
//! 那套写法的前提是 `ParsedCliArgs` 把子命令摊平成互斥的 `Option` 字段，于是只能
//! 「逐个问一遍你是不是这个命令」把信息猜回来。子命令现在原样保留在
//! `ParsedCliArgs::command`，因此新增子命令时编译器会在这里强制你处理。

#[cfg(feature = "web")]
#[path = "cli_run_serve.rs"]
mod cli_run_serve;

#[path = "cli_run_session.rs"]
mod cli_run_session;

#[path = "cli_run_serve_branch.rs"]
mod cli_run_serve_branch;

#[path = "cli_run_bench.rs"]
mod cli_run_bench;

use std::env;
use std::path::PathBuf;

use log::info;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{
    Commands, E2eCmd, McpCmd, McpSubCmd, ParsedCliArgs, PluginCmd, PluginInitCli, PluginListCli,
    PluginSubCmd, PluginValidateCli, SaveSessionCli, SaveSessionCmd, SseReplayCli, SseReplayCmd,
    ToolReplayCli, ToolReplayCmd, ToolReplaySubCmd, WebBearerCli, WebBearerCmd, WebBearerSubCmd,
    WorkflowCmd, WorkflowFileCli, WorkflowSubCmd,
};
use crate::config;
use crate::http_client;
use crate::observability;

/// `crabmate models` / `crabmate probe`：`bearer` 时仍要求进程环境变量 **`API_KEY`** 非空。
fn require_api_key_for_cli_models_probe(
    cfg: &config::AgentConfig,
) -> Result<String, std::io::Error> {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
        eprintln!(
            "请设置环境变量 API_KEY（当前 llm_http_auth_mode=bearer；models/probe 须从环境读取密钥）"
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "未设置环境变量 API_KEY",
        ));
    }
    Ok(v)
}

/// `serve` / `bench` 等：读取可选进程级 LLM Bearer（**仅环境变量 `API_KEY`**）。
///
/// 模型密钥权威在 Client 请求体 **`client_llm.api_key`**；服务端不再读 `client_llm` 钥匙串。
fn read_llm_api_key_from_env_lenient(cfg: &config::AgentConfig) -> String {
    let from_env = env::var("API_KEY").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return from_env;
    }
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer {
        info!(
            target: "crabmate",
            "API_KEY 未设置（llm_http_auth_mode=bearer）：对话请由 Client 发送 client_llm.api_key，或 export API_KEY 供 models/probe/进程回退"
        );
    }
    String::new()
}

fn apply_cli_llm_context_tokens_override(
    mut cfg: config::AgentConfig,
    cli_tokens: Option<u32>,
) -> config::AgentConfig {
    if let Some(n) = cli_tokens {
        cfg.llm_sampling.llm_context_tokens = n.min(10_000_000);
    }
    cfg
}

fn finalize_cli_config_path(explicit: Option<String>) -> Option<String> {
    if let Some(p) = explicit.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    config::resolve_interactive_cli_config_path(None).map(|p| p.to_string_lossy().into_owned())
}

fn load_cli_agent_config(
    config_path: Option<&str>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<config::AgentConfig, Box<dyn std::error::Error>> {
    let mut cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path)?,
        llm_context_tokens_cli,
    );
    crate::user_data::apply_user_data_llm_overrides(&mut cfg);
    crate::user_data::apply_user_data_tool_overrides(&mut cfg);
    crate::user_data::apply_user_data_web_api_bearer(&mut cfg);
    Ok(cfg)
}

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_cli_from_parsed(crate::parse_args()?).await
}

/// 已解析 argv：初始化日志，然后按子命令分派。
pub(super) async fn run_cli_from_parsed(
    args: ParsedCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    // serve 模式默认只输出 WARN/ERROR（避免 INFO 日志刷屏）；设 RUST_LOG=info 可恢复。
    observability::init_tracing_subscriber(
        args.global.log_file.as_deref().map(std::path::Path::new),
        /* quiet_cli_default */ true,
    )?;

    let ParsedCliArgs { mut global, command } = args;
    global.config_path = finalize_cli_config_path(global.config_path.take());
    dispatch(global, command).await
}

/// 顶层分派：`serve` / `bench` 需要会话初始化（HTTP client、工具表、进程句柄），其余不需要。
async fn dispatch(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Serve(serve) => run_session(global, Commands::Serve(serve)).await,
        Commands::Bench(bench) => run_session(global, Commands::Bench(bench)).await,
        other => dispatch_offline(global, other).await,
    }
}

/// 不初始化会话句柄的运维子命令（诊断 / 离线产物）。
async fn dispatch_offline(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Doctor => run_doctor(&global),
        Commands::Config => run_dry_run(&global).await,
        Commands::Models => run_models_or_probe(&global, false).await,
        Commands::Probe => run_models_or_probe(&global, true).await,
        Commands::WebBearer(cmd) => run_web_bearer(cmd),
        Commands::SaveSession(cmd) => run_save_session(&global, cmd),
        Commands::SseReplay(cmd) => run_sse_replay(cmd),
        other => dispatch_toolish(global, other).await,
    }
}

/// 需要配置与 HTTP 客户端，但不进 `serve` / `bench` 主循环的子命令。
async fn dispatch_toolish(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::ToolReplay(cmd) => run_tool_replay(&global, cmd).await,
        Commands::Plugin(cmd) => run_plugin(&global, cmd),
        Commands::Workflow(cmd) => run_workflow(&global, cmd).await,
        Commands::Mcp(cmd) => run_mcp(&global, cmd).await,
        Commands::E2e(cmd) => run_e2e(&global, cmd).await,
        // `serve` / `bench` 已在 `dispatch` 分走，其余在 `dispatch_offline` 分走。
        // 兜底返回用户可读的用法错误（而不是 `unreachable!()`）：真走到这里说明
        // 分派表漏了一层，报错比 panic 更容易定位。
        _ => Err(unspecified_subcommand_error()),
    }
}

fn unspecified_subcommand_error() -> Box<dyn std::error::Error> {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "未指定可执行的子命令。请使用 `crabmate serve`（API）或运维子命令（如 `doctor`）；官方终端为 Client 仓 `crabmate-tui`。同进程 `chat|repl|tui` 入口已移除。",
    )
    .into()
}

fn run_doctor(global: &CliGlobals) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    crate::runtime::cli_doctor::print_doctor_report(&cfg, global.workspace.as_deref());
    Ok(())
}

async fn run_dry_run(global: &CliGlobals) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    let key_note = match cfg.llm.llm_http_auth_mode {
        config::LlmHttpAuthMode::None => "llm_http_auth_mode=none（API_KEY 可选）".to_string(),
        config::LlmHttpAuthMode::Bearer => {
            let env_ok = env::var("API_KEY")
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if env_ok {
                "llm_http_auth_mode=bearer 且 API_KEY 非空".to_string()
            } else {
                "llm_http_auth_mode=bearer：当前未设置 API_KEY（对话靠 Client client_llm.api_key；models/probe 仍须环境变量）"
                    .to_string()
            }
        }
    };
    println!("配置检查通过：{}（纯 API，不托管 UI）", key_note);
    Ok(())
}

async fn run_models_or_probe(
    global: &CliGlobals,
    probe: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    let api_key = require_api_key_for_cli_models_probe(&cfg)?;
    let client = http_client::build_shared_api_client(&cfg.llm_http_retry)?;
    if probe {
        crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
    } else {
        crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
    }
    Ok(())
}

fn run_web_bearer(cmd: WebBearerCmd) -> Result<(), Box<dyn std::error::Error>> {
    let cli = match cmd.sub {
        WebBearerSubCmd::Status => WebBearerCli::Status,
        WebBearerSubCmd::Set(s) => WebBearerCli::Set {
            token: s.token,
            stdin: s.stdin,
            from_env: s.from_env,
        },
        WebBearerSubCmd::Clear => WebBearerCli::Clear,
    };
    crate::runtime::cli_web_bearer::run_web_bearer_command(cli)?;
    Ok(())
}

fn run_save_session(
    global: &CliGlobals,
    cmd: SaveSessionCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    let cli = SaveSessionCli {
        format: cmd.format,
        projection: cmd.projection,
        session_file: cmd.session_file,
    };
    crate::runtime::cli::run_save_session_command(&cfg, &global.workspace, cli)?;
    Ok(())
}

fn run_sse_replay(cmd: SseReplayCmd) -> Result<(), Box<dyn std::error::Error>> {
    crate::runtime::cli::run_sse_replay_command(SseReplayCli {
        file: cmd.file,
        format: cmd.format,
        job_id: cmd.job_id,
    })?;
    Ok(())
}

async fn run_tool_replay(
    global: &CliGlobals,
    cmd: ToolReplayCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    let cli = match cmd.sub {
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
    crate::runtime::cli::run_tool_replay_command(&cfg, &global.workspace, cli).await?;
    Ok(())
}

fn run_plugin(global: &CliGlobals, cmd: PluginCmd) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    match cmd.sub {
        PluginSubCmd::Init(i) => crate::runtime::cli::run_plugin_init_command(
            &cfg,
            &global.workspace,
            PluginInitCli {
                name: i.name,
                description: i.description,
                command: i.command,
                args: i.args,
                pass_args_json: i.pass_args_json,
                output: i.output,
            },
        )?,
        PluginSubCmd::Validate(v) => crate::runtime::cli::run_plugin_validate_command(
            &cfg,
            &global.workspace,
            PluginValidateCli {
                file: v.file,
                json: v.json,
                jsonl: v.jsonl,
            },
        )?,
        PluginSubCmd::List(l) => crate::runtime::cli::run_plugin_list_command(
            &cfg,
            &global.workspace,
            PluginListCli {
                file: l.file,
                json: l.json,
                jsonl: l.jsonl,
            },
        )?,
    }
    Ok(())
}

async fn run_workflow(
    global: &CliGlobals,
    cmd: WorkflowCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    fn usage_err<E: Into<String>>(e: E) -> Box<dyn std::error::Error> {
        Box::new(crate::CliExitError::new(2, e))
    }
    match cmd.sub {
        // 校验 / 编译不读配置：只做作者层 YAML/Markdown 的静态检查。
        WorkflowSubCmd::Validate(v) => {
            crate::runtime::cli_workflow::run_workflow_validate_command(&WorkflowFileCli {
                file: v.file,
                json: v.json,
            })
            .map_err(usage_err)?;
        }
        WorkflowSubCmd::Compile(c) => {
            crate::runtime::cli_workflow::run_workflow_compile_command(&WorkflowFileCli {
                file: c.file,
                json: c.json,
            })
            .map_err(usage_err)?;
        }
        WorkflowSubCmd::Run(r) => {
            let cfg =
                load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
            crate::runtime::cli_workflow::run_workflow_run_command(
                &cfg,
                &global.workspace,
                WorkflowFileCli {
                    file: r.file,
                    json: r.json,
                },
            )
            .await
            .map_err(usage_err)?;
        }
    }
    Ok(())
}

async fn run_mcp(global: &CliGlobals, cmd: McpCmd) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    match cmd.sub {
        McpSubCmd::List(l) => crate::runtime::cli_mcp::run_mcp_list(&cfg, l.probe).await,
        McpSubCmd::Serve(s) => {
            crate::runtime::cli_mcp::run_mcp_serve(&cfg, &global.workspace, s.no_tools, s.port)
                .await
                .map_err(std::io::Error::other)?
        }
    }
    Ok(())
}

async fn run_e2e(global: &CliGlobals, cmd: E2eCmd) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    let api_key = read_llm_api_key_from_env_lenient(&cfg);

    let mode = match cmd.mode.as_str() {
        "record" => crate::cm_llm::E2eMode::Record,
        "replay" => crate::cm_llm::E2eMode::Replay,
        _ => crate::cm_llm::E2eMode::Real,
    };

    let artifacts_root = cmd
        .output_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".crabmate/e2e_artifacts"));
    let recordings_dir = cmd
        .recordings_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/llm_recordings"));
    let scenarios_file = cmd.scenarios_file.as_deref().map(PathBuf::from);

    let run_cfg = crate::e2e_scenario::E2eRunConfig {
        api_key,
        artifacts_root,
        recordings_dir,
        mode,
        judge_config: crate::e2e_scenario::JudgeConfig {
            enabled: cmd.judge,
            ..Default::default()
        },
    };
    crate::e2e_scenario::run_e2e_cli(&run_cfg, scenarios_file.as_deref()).await?;
    Ok(())
}

/// `serve` / `bench`：载入配置、初始化会话句柄，再由 `cli_run_session` 分两条主路径。
async fn run_session(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = load_cli_agent_config(global.config_path.as_deref(), global.llm_context_tokens)?;
    if let Commands::Serve(serve) = &command
        && serve.yes
    {
        cfg.command_exec.allowed_commands = std::sync::Arc::from(vec!["*".to_string()]);
        log::warn!(
            target: "crabmate",
            "serve --yes：已跳过命令白名单（allowed_commands → \"*\"），run_command 不再校验命令名；仅请在完全信任或已隔离的环境使用"
        );
    }
    let session = cli_run_session::init_cli_session_start(cfg, global.no_tools).await?;

    Box::pin(cli_run_session::run_cli_main_routes(
        cli_run_session::CliDispatchArgs {
            session,
            global,
            command,
        },
    ))
    .await
}
