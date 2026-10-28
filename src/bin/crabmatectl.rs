//! `crabmatectl`：本地多实例 `crabmate serve` 进程管理（Unix-only）。
//!
//! **独立二进制而非 `crabmate ctl` 子命令**：它管理的是别的进程、不是一次 agent
//! 调用，入口分开才能一眼看出用途；同时也不让 `serve` 的 CLI 面被运维动作撑大。
//!
//! 仍然是同一个 crate、同一个 `server` feature 构建，所以「ctl 与 serve 版本一致」
//! 不依赖发布流程，而是结构上不可能不一致。
//!
//! 退出码沿用 `cm_runtime::cli_exit` 的既有约定：1 一般失败、2 用法错误；细分原因
//! 在 [`crabmate::cm_ctl::CtlError::error_code`] 里。

use clap::Parser;

use crabmate::config::cli::ctl::{CtlCmd, CtlSubCmd};

/// `crabmatectl <subcommand>`。
#[derive(Parser, Debug)]
#[command(
    name = "crabmatectl",
    version,
    about = "本地多实例 crabmate serve 进程管理（Unix-only）",
    long_about = None,
)]
struct CtlCli {
    /// 实例 workspace 路径（`up` / `run` 必填；其余子命令忽略）
    #[arg(long, global = true, value_name = "PATH")]
    workspace: Option<String>,

    #[command(subcommand)]
    sub: CtlSubCmd,
}

fn main() {
    let cli = CtlCli::parse();
    let cmd = CtlCmd { sub: cli.sub };

    #[cfg(unix)]
    {
        if let Err(e) = crabmate::cm_ctl::run(&cmd, cli.workspace.as_deref()) {
            eprintln!("{e}");
            std::process::exit(e.exit_code);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (&cmd, cli.workspace.as_deref());
        eprintln!("crabmatectl 目前仅支持 Unix（Linux/macOS）");
        std::process::exit(2);
    }
}
