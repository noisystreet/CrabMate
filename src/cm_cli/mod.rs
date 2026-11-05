//! CLI 分派表：**只有分派，没有业务**（S5，设计 `docs/design/sdk_facade.md` §6；#1295）。
//!
//! 三条结构约束，由脚本而非注释兜住：
//!
//! 1. `cm_cli/*.rs` 单文件 ≤ 200 行、**每命令一个模块**（`scripts/fn_nloc_rust_metrics.py`）；
//! 2. `cm_cli/` **不得**引用内部实现门面 `cm_internal`（`scripts/check-crate-deps.sh`）——
//!    分派层只认「子命令 → 执行体」，需要内部能力时由执行体自己取，别让 CLI 层长成
//!    第二个内部门面；
//! 3. 命令内部的装配细节放子目录（如 `cm_cli/serve/`），仍受全局 920 行/文件上限。
//!
//! [`CliContext`] **刻意不持有已加载的配置**：`web-bearer` / `sse-replay` 这类命令不读配置，
//! 配置损坏时也应能跑（现状语义，勿改成「总是加载」）。需要配置的模块自己调
//! [`config_load::load_cli_agent_config`]。

mod config_load;
mod diagnostics;
mod offline;
mod session;
mod tooling;
mod tooling_net;

#[cfg(feature = "web")]
mod serve;

mod bench;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{Commands, ParsedCliArgs};

/// 分派上下文：全局选项 + 子命令原样。
pub(crate) struct CliContext {
    pub(crate) global: CliGlobals,
    pub(crate) command: Commands,
}

impl CliContext {
    pub(crate) fn from_parsed(args: ParsedCliArgs) -> Self {
        Self {
            global: args.global,
            command: args.command,
        }
    }
}

/// 顶层分派：`serve` / `bench` 需要会话初始化（HTTP client、工具表、进程句柄），其余不需要。
pub(crate) async fn dispatch(mut ctx: CliContext) -> Result<(), Box<dyn std::error::Error>> {
    ctx.global.config_path = config_load::finalize_cli_config_path(ctx.global.config_path.take());
    let CliContext { global, command } = ctx;
    match command {
        Commands::Serve(serve) => session::run(global, Commands::Serve(serve)).await,
        Commands::Bench(bench) => session::run(global, Commands::Bench(bench)).await,
        other => dispatch_offline(global, other).await,
    }
}

/// 不初始化会话句柄的运维子命令（诊断 / 离线产物）。
async fn dispatch_offline(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Doctor => diagnostics::run_doctor(&global),
        Commands::Config => diagnostics::run_dry_run(&global).await,
        Commands::Models => diagnostics::run_models_or_probe(&global, false).await,
        Commands::Probe => diagnostics::run_models_or_probe(&global, true).await,
        Commands::WebBearer(cmd) => offline::run_web_bearer(cmd),
        Commands::SaveSession(cmd) => offline::run_save_session(&global, cmd),
        Commands::SseReplay(cmd) => offline::run_sse_replay(cmd),
        other => dispatch_toolish(global, other).await,
    }
}

/// 需要配置与 HTTP 客户端，但不进 `serve` / `bench` 主循环的子命令。
async fn dispatch_toolish(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::ToolReplay(cmd) => tooling::run_tool_replay(&global, cmd).await,
        Commands::Plugin(cmd) => tooling::run_plugin(&global, cmd),
        Commands::Workflow(cmd) => tooling_net::run_workflow(&global, cmd).await,
        Commands::Mcp(cmd) => tooling_net::run_mcp(&global, cmd).await,
        Commands::E2e(cmd) => tooling_net::run_e2e(&global, cmd).await,
        // `serve` / `bench` 已在 `dispatch` 分走，其余在 `dispatch_offline` 分走。
        // 兜底返回用户可读的用法错误（而不是 `unreachable!()`）：真走到这里说明分派表
        // 漏了一层，报错比 panic 更容易定位。
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
