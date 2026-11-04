//! `cargo run` / 库入口 [`crate::run`] 的 CLI 编排（从 `lib.rs` 拆出以降低单函数圈复杂度）。

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

use crate::config;
use crate::config::cli::{
    E2eCliArgs, ExtraCliCommand, ParsedCliArgs, PluginInitCli, PluginListCli, PluginValidateCli,
    SaveSessionCli, WebBearerCli, WorkflowFileCli, parse_args,
};
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

struct EarlyCliDispatch<'a> {
    config_path: &'a Option<String>,
    workspace_cli: &'a Option<String>,
    extra_cli: ExtraCliCommand,
    save_session: Option<SaveSessionCli>,
    web_bearer: Option<WebBearerCli>,
    tool_replay: Option<crate::config::cli::ToolReplayCli>,
    sse_replay: Option<crate::config::cli::SseReplayCli>,
    plugin_init: Option<PluginInitCli>,
    plugin_validate: Option<PluginValidateCli>,
    plugin_list: Option<PluginListCli>,
    workflow_validate: Option<WorkflowFileCli>,
    workflow_compile: Option<WorkflowFileCli>,
    workflow_run: Option<WorkflowFileCli>,
    e2e: Option<E2eCliArgs>,
}

fn try_early_web_bearer(d: &EarlyCliDispatch<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(cmd) = d.web_bearer.clone() else {
        return Ok(false);
    };
    crate::runtime::cli_web_bearer::run_web_bearer_command(cmd)?;
    Ok(true)
}

fn try_early_save_session(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(ss) = d.save_session.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli::run_save_session_command(&cfg, d.workspace_cli, ss)?;
    Ok(true)
}

async fn try_early_tool_replay(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(tr) = d.tool_replay.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli::run_tool_replay_command(&cfg, d.workspace_cli, tr).await?;
    Ok(true)
}

fn try_early_sse_replay(d: &EarlyCliDispatch<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(sr) = d.sse_replay.clone() else {
        return Ok(false);
    };
    crate::runtime::cli::run_sse_replay_command(sr)?;
    Ok(true)
}

fn try_early_plugin_init(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pi) = d.plugin_init.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli::run_plugin_init_command(&cfg, d.workspace_cli, pi)?;
    Ok(true)
}

fn try_early_plugin_validate(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pv) = d.plugin_validate.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli::run_plugin_validate_command(&cfg, d.workspace_cli, pv)?;
    Ok(true)
}

fn try_early_plugin_list(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pl) = d.plugin_list.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli::run_plugin_list_command(&cfg, d.workspace_cli, pl)?;
    Ok(true)
}

fn try_early_workflow_validate(
    d: &EarlyCliDispatch<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wv) = d.workflow_validate.clone() else {
        return Ok(false);
    };
    crate::runtime::cli_workflow::run_workflow_validate_command(&wv)
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

fn try_early_workflow_compile(
    d: &EarlyCliDispatch<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wc) = d.workflow_compile.clone() else {
        return Ok(false);
    };
    crate::runtime::cli_workflow::run_workflow_compile_command(&wc)
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

async fn try_early_workflow_run(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wr) = d.workflow_run.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    crate::runtime::cli_workflow::run_workflow_run_command(&cfg, d.workspace_cli, wr)
        .await
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

async fn try_early_e2e(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(e2e) = d.e2e.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
    let api_key = read_llm_api_key_from_env_lenient(&cfg);

    let mode = match e2e.mode.as_str() {
        "record" => crate::cm_llm::E2eMode::Record,
        "replay" => crate::cm_llm::E2eMode::Replay,
        _ => crate::cm_llm::E2eMode::Real,
    };

    let artifacts_root = e2e
        .output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".crabmate/e2e_artifacts"));
    let recordings_dir = e2e
        .recordings_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/llm_recordings"));

    let scenarios_file = e2e.scenarios_file.map(PathBuf::from);

    let run_cfg = crate::e2e_scenario::E2eRunConfig {
        api_key,
        artifacts_root,
        recordings_dir,
        mode,
        judge_config: crate::e2e_scenario::JudgeConfig {
            enabled: e2e.judge,
            ..Default::default()
        },
    };
    crate::e2e_scenario::run_e2e_cli(&run_cfg, scenarios_file.as_deref()).await?;
    Ok(true)
}

/// `doctor` / `mcp list` / `mcp serve`：由 [`ExtraCliCommand`] 分流。
async fn try_dispatch_early_extra_cli(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<Option<bool>, Box<dyn std::error::Error>> {
    match d.extra_cli {
        ExtraCliCommand::Doctor => {
            let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
            crate::runtime::cli_doctor::print_doctor_report(&cfg, d.workspace_cli.as_deref());
            Ok(Some(true))
        }
        ExtraCliCommand::McpList { probe } => {
            let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
            crate::runtime::cli_mcp::run_mcp_list(&cfg, probe).await;
            Ok(Some(true))
        }
        ExtraCliCommand::McpServe { no_tools, port } => {
            let cfg = load_cli_agent_config(d.config_path.as_deref(), tokens)?;
            crate::runtime::cli_mcp::run_mcp_serve(&cfg, d.workspace_cli, no_tools, port)
                .await
                .map_err(std::io::Error::other)?;
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

/// `save-session` / `plugin *` 等非默认主流程（同步）。
fn try_dispatch_early_workspace_commands(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<Option<bool>, Box<dyn std::error::Error>> {
    type DispatchFn =
        fn(&EarlyCliDispatch<'_>, Option<u32>) -> Result<bool, Box<dyn std::error::Error>>;
    type DispatchFnNoTokens = fn(&EarlyCliDispatch<'_>) -> Result<bool, Box<dyn std::error::Error>>;
    // 接受 tokens 参数的子命令
    let with_tokens: &[DispatchFn] = &[
        try_early_save_session,
        try_early_plugin_init,
        try_early_plugin_validate,
        try_early_plugin_list,
    ];
    for f in with_tokens {
        if f(d, tokens)? {
            return Ok(Some(true));
        }
    }
    // 不含 tokens 的子命令
    let no_tokens: &[DispatchFnNoTokens] = &[
        try_early_web_bearer,
        try_early_sse_replay,
        try_early_workflow_validate,
        try_early_workflow_compile,
    ];
    for f in no_tokens {
        if f(d)? {
            return Ok(Some(true));
        }
    }
    Ok(None)
}

/// 组合分流：`tool-replay`（异步）与 `save-session` / `plugin *` 等（同步）。
///
/// 二者合并为单一 helper，避免 `run_early_commands` 中重复的 `if ... ?` 抬升圈复杂度。
async fn try_dispatch_early_tool_replay_and_workspace(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if try_early_tool_replay(d, tokens).await? {
        return Ok(true);
    }
    Ok(try_dispatch_early_workspace_commands(d, tokens)?.is_some())
}

async fn run_early_commands(
    d: EarlyCliDispatch<'_>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(true) = try_dispatch_early_extra_cli(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    if try_dispatch_early_tool_replay_and_workspace(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    if try_early_workflow_run(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    if try_early_e2e(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    Ok(false)
}

async fn run_dry_run(
    config_path: &Option<String>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(config_path.as_deref(), llm_context_tokens_cli)?;
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
    config_path: &Option<String>,
    extra_cli: ExtraCliCommand,
    llm_context_tokens_cli: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_cli_agent_config(config_path.as_deref(), llm_context_tokens_cli)?;
    let api_key = require_api_key_for_cli_models_probe(&cfg)?;
    let client = http_client::build_shared_api_client(&cfg.llm_http_retry)?;
    if extra_cli == ExtraCliCommand::Models {
        crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
    } else {
        crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
    }
    Ok(())
}

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_cli_from_parsed(parse_args()?).await
}

/// 已解析 argv：初始化日志并处理 `doctor` / `save-session` 等早退子命令。
pub(super) async fn run_cli_from_parsed(
    mut args: ParsedCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    // serve 模式默认只输出 WARN/ERROR（避免 INFO 日志刷屏）；设 RUST_LOG=info 可恢复。
    observability::init_tracing_subscriber(
        args.log_file.as_deref().map(std::path::Path::new),
        /* quiet_cli_default */ true,
    )?;

    args.config_path = finalize_cli_config_path(args.config_path.take());

    if run_early_commands(
        EarlyCliDispatch {
            config_path: &args.config_path,
            workspace_cli: &args.workspace_cli,
            extra_cli: args.extra_cli,
            save_session: args.save_session.clone(),
            web_bearer: args.web_bearer.clone(),
            tool_replay: args.tool_replay.clone(),
            sse_replay: args.sse_replay.clone(),
            plugin_init: args.plugin_init.clone(),
            plugin_validate: args.plugin_validate.clone(),
            plugin_list: args.plugin_list.clone(),
            workflow_validate: args.workflow_validate.clone(),
            workflow_compile: args.workflow_compile.clone(),
            workflow_run: args.workflow_run.clone(),
            e2e: args.e2e.clone(),
        },
        args.llm_context_tokens_cli,
    )
    .await?
    {
        return Ok(());
    }

    Box::pin(run_cli_default_main(args)).await
}

/// 默认主路径：`config`（一次性配置检查）、`models`/`probe`，或 `serve` / `bench`。
async fn run_cli_default_main(args: ParsedCliArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.dry_run {
        run_dry_run(&args.config_path, args.llm_context_tokens_cli).await?;
        return Ok(());
    }

    let cfg = load_cli_agent_config(args.config_path.as_deref(), args.llm_context_tokens_cli)?;

    if matches!(
        args.extra_cli,
        ExtraCliCommand::Models | ExtraCliCommand::Probe
    ) {
        run_models_or_probe(
            &args.config_path,
            args.extra_cli,
            args.llm_context_tokens_cli,
        )
        .await?;
        return Ok(());
    }

    Box::pin(run_cli_interactive_session(args, cfg)).await
}

/// 载入配置并进入 `serve` / `bench` 分发。
async fn run_cli_interactive_session(
    args: ParsedCliArgs,
    mut cfg: config::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.serve_yes {
        cfg.command_exec.allowed_commands = std::sync::Arc::from(vec!["*".to_string()]);
        log::warn!(
            target: "crabmate",
            "serve --yes：已跳过命令白名单（allowed_commands → \"*\"），run_command 不再校验命令名；仅请在完全信任或已隔离的环境使用"
        );
    }
    let session = cli_run_session::init_cli_session_start(cfg, args.no_tools).await?;

    Box::pin(cli_run_session::run_cli_main_routes(
        cli_run_session::CliDispatchArgs {
            session,
            config_path: args.config_path,
            serve_port: args.serve_port,
            serve_desktop_ready_json: args.serve_desktop_ready_json,
            serve_workspace_unset: args.serve_workspace_unset,
            http_bind_host: args.http_bind_host,
            workspace_cli: args.workspace_cli,
            bench_args: args.bench_args,
        },
    ))
    .await
}
