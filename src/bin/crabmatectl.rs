//! `crabmatectl`：本地多实例 `crabmate serve` 进程管理（Linux-only）。
//!
//! **平台边界**：进程身份校验与存活判定读 `/proc`，故仅 Linux。其他 Unix（macOS）
//! 能编译也会被 `cm_ctl::PLATFORM_SUPPORTED` 在入口拒绝——`pid_alive` 在那里恒为
//! `false`，放行等于让 `down` 清掉记录却不杀进程。
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

// `CtlCli` 定义在 lib（`cm_config::cli::ctl`）而不是本文件：`crabmate-gen-man` 是另一个
// bin crate 入口，只有共享同一份定义才能生成 `man/crabmatectl.1` 而不抄一份会漂移的副本。
use crabmate::config::cli::ctl::{CtlCli, CtlCmd};

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
        eprintln!("crabmatectl 目前仅支持 Linux：进程身份与存活判定依赖 procfs（/proc）");
        std::process::exit(2);
    }
}
