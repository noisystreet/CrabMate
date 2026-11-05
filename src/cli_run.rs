//! `cargo run` / 库入口 [`crate::run`] 的 CLI 入口：初始化日志 → 交给 `cm_cli` 分派。
//!
//! 具体命令在 `src/cm_cli/` 里，一个命令一个模块（S5，见 #1295）。本文件只剩
//! 「日志初始化 + 交给分派表」——任何子命令逻辑都不该加在这里。

use crate::cm_cli::{CliContext, dispatch};
use crate::cm_config::cli::definitions::ParsedCliArgs;
use crate::observability;

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、初始化日志、按子命令分派。
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
    dispatch(CliContext::from_parsed(args)).await
}
