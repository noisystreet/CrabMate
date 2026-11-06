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

/// 不进 `serve` / `bench` 主循环的子命令：诊断、离线产物、需要配置与 HTTP 客户端的运维动作。
///
/// **一个穷尽的 `match`**：新增 `Commands` 变体时这里编译失败，而不是静默滑进某个兜底。
/// `serve` / `bench` 已在 [`dispatch`] 分走，落到本函数只可能是分派表被改坏。
async fn dispatch_offline(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = command.name();
    match command {
        // 不读配置也能跑（配置损坏时依然可用）。
        Commands::WebBearer(cmd) => offline::run_web_bearer(cmd),
        Commands::SseReplay(cmd) => offline::run_sse_replay(cmd),
        // 自己按需加载配置。
        Commands::Doctor => diagnostics::run_doctor(&global),
        Commands::Config => diagnostics::run_dry_run(&global).await,
        Commands::Models => diagnostics::run_models_or_probe(&global, false).await,
        Commands::Probe => diagnostics::run_models_or_probe(&global, true).await,
        Commands::SaveSession(cmd) => offline::run_save_session(&global, cmd),
        // 需要配置 + HTTP 客户端（仍不建工具表 / 进程句柄）。
        Commands::ToolReplay(cmd) => tooling::run_tool_replay(&global, cmd).await,
        Commands::Plugin(cmd) => tooling::run_plugin(&global, cmd),
        Commands::Workflow(cmd) => tooling_net::run_workflow(&global, cmd).await,
        Commands::Mcp(cmd) => tooling_net::run_mcp(&global, cmd).await,
        Commands::E2e(cmd) => tooling_net::run_e2e(&global, cmd).await,
        // **不写通配臂**：上面已把 12 个变体逐一接上，只剩这两个被 `dispatch` 提前分走。
        // 新增变体会让这条穷尽匹配编译失败——那才是「编译器强制你处理」。
        Commands::Serve(_) | Commands::Bench(_) => Err(dispatch_table_gap(name)),
    }
}

/// `Commands` 有变体却没接上执行体：**分派表缺口**（程序缺陷），不是用户用法错误。
///
/// 别退回「未指定可执行的子命令」那类文案：`Commands` 是 clap 必选子命令、没有空变体，
/// 用户根本没有「不给子命令」这条路，那样写只会把维护者引向错误方向。
pub(super) fn dispatch_table_gap(name: &'static str) -> Box<dyn std::error::Error> {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "子命令 `{name}` 没有接入 CLI 分派表（src/cm_cli/mod.rs），这是程序缺陷而非用法问题；请提 issue 或就地补上执行体。"
        ),
    )
    .into()
}
