//! `crabmate ctl` 的 clap 定义（执行在 `crate::cm_ctl`）。
//!
//! 注意两个 `--config` 的区别：
//!
//! - 全局 `--config`（`GlobalOpts`）：**ctl 自己**读的配置，仅用于 `workspace` 预检；
//! - 子命令 `--serve-config`：交给**子 serve** 的 `--config`。
//!
//! 二者绝不互相继承 —— 否则子实例会静默套用 ctl 的配置，形成「看起来隔离、实际同源」。

use clap::{Parser, Subcommand};

/// `ctl` 子命令容器。
#[derive(Parser, Debug, Clone)]
pub struct CtlCmd {
    #[command(subcommand)]
    pub sub: CtlSubCmd,
}

/// `ctl` 的各个动作。
#[derive(Subcommand, Debug, Clone)]
pub enum CtlSubCmd {
    /// 启动一个 serve 实例（脱管：ctl 退出后继续运行）
    Up(UpCmd),
    /// 停止实例（对整组发 SIGTERM → 宽限 → SIGKILL）
    Down(DownCmd),
    /// 重启实例（down + up，复用原端口与配置）
    Restart(RestartCmd),
    /// 前台运行（阻塞；便于 systemd Type=simple / docker CMD 直接托管）
    Run(RunCmd),
    /// 列出全部实例
    Ls(LsCmd),
    /// 显示单个实例详情
    Status(StatusCmd),
    /// 查看实例日志
    Logs(LogsCmd),
    /// 清理 stale / 已退出 / 失败的记录（不碰健康实例）
    Prune(PruneCmd),
}

/// 启动实例。
#[derive(Parser, Debug, Clone)]
pub struct UpCmd {
    /// 实例名（仅字母数字与 `.` `_` `-`）
    pub name: String,

    /// 交给**子 serve** 的 `--config`（不是 ctl 自己的 `--config`）；省略则让 serve 按自身 XDG 顺序解析
    #[arg(long = "serve-config", value_name = "FILE")]
    pub serve_config: Option<String>,

    /// 指定端口；省略则在端口池（默认 8080-8089）内取第一个空闲
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// 跳过子 serve 的命令白名单（等价 `serve --yes`）。**绝不**从 ctl 隐式继承
    #[arg(long)]
    pub yes: bool,

    /// 等待 TCP 就绪的秒数
    #[arg(long = "timeout", value_name = "SECS", default_value_t = 10)]
    pub timeout_secs: u64,
}

/// 停止实例。
#[derive(Parser, Debug, Clone)]
pub struct DownCmd {
    /// 实例名；与 `--all` 二选一
    pub name: Option<String>,

    /// 停止全部实例
    #[arg(long)]
    pub all: bool,

    /// 终止宽限秒数（超时后用 SIGKILL）
    #[arg(long = "timeout", value_name = "SECS", default_value_t = 10)]
    pub timeout_secs: u64,
}

/// 重启实例。
#[derive(Parser, Debug, Clone)]
pub struct RestartCmd {
    /// 实例名
    pub name: String,

    /// 等待 TCP 就绪的秒数
    #[arg(long = "timeout", value_name = "SECS", default_value_t = 10)]
    pub timeout_secs: u64,
}

/// 前台运行实例。
#[derive(Parser, Debug, Clone)]
pub struct RunCmd {
    /// 实例名
    pub name: String,

    /// 交给子 serve 的 `--config`
    #[arg(long = "serve-config", value_name = "FILE")]
    pub serve_config: Option<String>,

    /// 指定端口
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// 跳过子 serve 的命令白名单
    #[arg(long)]
    pub yes: bool,
}

/// 列出实例。
#[derive(Parser, Debug, Clone)]
pub struct LsCmd {
    /// 输出 JSON（字段稳定，供脚本与客户端消费）
    #[arg(long)]
    pub json: bool,
}

/// 实例详情。
#[derive(Parser, Debug, Clone)]
pub struct StatusCmd {
    /// 实例名
    pub name: String,

    /// 输出 JSON
    #[arg(long)]
    pub json: bool,
}

/// 查看日志。
#[derive(Parser, Debug, Clone)]
pub struct LogsCmd {
    /// 实例名
    pub name: String,

    /// 跟随输出（等价 tail -f）
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// 仅显示末尾 N 行
    #[arg(long, value_name = "N", default_value_t = 200)]
    pub tail: usize,
}

/// 清理陈旧记录。
#[derive(Parser, Debug, Clone)]
pub struct PruneCmd {
    /// 只列出将清理的记录，不实际删除
    #[arg(long)]
    pub dry_run: bool,
}
