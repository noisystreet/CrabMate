//! `crabmatectl` 的 clap 定义（执行在 `crate::cm_ctl`）。
//!
//! 注意两个 `--config` 的区别：
//!
//! - 全局 `--config`（`GlobalOpts`）：**ctl 自己**读的配置，仅用于 `workspace` 预检；
//! - 子命令 `--serve-config`：交给**子 serve** 的 `--config`。
//!
//! 二者绝不互相继承 —— 否则子实例会静默套用 ctl 的配置，形成「看起来隔离、实际同源」。

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

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
    /// 按 `instances.toml` 声明收敛实例（起该起的、停该停的）
    Supervise(SuperviseCmd),
}

/// `--restart` 的取值。
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartArg {
    /// 默认：只在声明变化时收敛，绝不因崩溃重启。
    Never,
    /// 观测到 failed / stopped / stale 时按退避重启。
    OnFailure,
}

/// 按声明文件收敛实例。
#[derive(Parser, Debug, Clone)]
pub struct SuperviseCmd {
    /// 只监督这一个实例（**必须出现在声明里**）；省略则收敛整份声明
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// 从声明文件收敛（`$XDG_CONFIG_HOME/crabmate/instances.toml`）
    #[arg(long = "from-config")]
    pub from_config: bool,

    /// 声明文件路径（默认 `$XDG_CONFIG_HOME/crabmate/instances.toml`）
    #[arg(long = "instances-file", value_name = "FILE")]
    pub instances_file: Option<String>,

    /// 只打印计划，不启动也不回收（无副作用）
    #[arg(long)]
    pub dry_run: bool,

    /// 监听声明变化并热生效（常驻；Ctrl-C 退出）
    #[arg(long)]
    pub watch: bool,

    /// `--watch` 的轮询间隔秒数（不用 inotify：容器 overlayfs 不可靠）
    #[arg(long = "interval", value_name = "SECS", default_value_t = 1)]
    pub interval_secs: u64,

    /// 同时提供本地控制面（unix socket）：`add` / `remove` / `list` / `disable` / `enable`，仅同用户可连
    ///
    /// 省略值时为 `$XDG_RUNTIME_DIR/crabmate/ctl.sock`。给出即常驻（Ctrl-C 退出）。
    #[arg(
        long = "socket",
        value_name = "PATH",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub socket: Option<String>,

    /// 崩溃后是否重启：`never`（默认，只在声明变化时收敛）/ `on-failure`（按退避重启）
    ///
    /// `on-failure` 只在常驻模式（`--watch` / `--socket`）下有意义。
    #[arg(long = "restart", value_enum, default_value_t = RestartArg::Never)]
    pub restart: RestartArg,

    /// `on-failure` 的健康巡检间隔秒数（与 `--interval` 的声明轮询分开）
    #[arg(long = "health-interval", value_name = "SECS", default_value_t = 30)]
    pub health_interval_secs: u64,

    /// 首次退避秒数（之后按 2 倍增长，直到 `--backoff-max`）
    #[arg(long = "backoff-initial", value_name = "SECS", default_value_t = 5)]
    pub backoff_initial_secs: u64,

    /// 退避封顶秒数（永不放弃，只把间隔封顶）
    #[arg(long = "backoff-max", value_name = "SECS", default_value_t = 300)]
    pub backoff_max_secs: u64,

    /// 启动时等待 TCP 就绪的秒数
    #[arg(long = "timeout", value_name = "SECS", default_value_t = 10)]
    pub timeout_secs: u64,

    /// 回收旧实例时的终止宽限秒数
    #[arg(long = "grace", value_name = "SECS", default_value_t = 10)]
    pub grace_secs: u64,
}

/// 启动实例。
#[derive(Parser, Debug, Clone)]
pub struct UpCmd {
    /// 实例名；与 `--all` 二选一
    pub name: Option<String>,

    /// 按 `instances.toml` 声明批量启动（**只启动，不回收**：回收归 `supervise`）
    #[arg(long)]
    pub all: bool,

    /// 声明文件路径（默认 `$XDG_CONFIG_HOME/crabmate/instances.toml`；仅 `--all` 使用）
    #[arg(long = "instances-file", value_name = "FILE")]
    pub instances_file: Option<String>,

    /// 只打印计划，不实际启动（仅 `--all` 使用）
    #[arg(long)]
    pub dry_run: bool,

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

    /// 终止旧实例时的宽限秒数（宽限内未退出则 SIGKILL）
    ///
    /// 与 `down --timeout` / `supervise --grace` 同义。`serve` 收到 `SIGTERM` 后
    /// 可能几十秒才退出，而 `restart` 又恰是 profile 变更的官方补救路径 ——
    /// 没有这个口子，它会稳定地走 SIGKILL。
    #[arg(long = "grace", value_name = "SECS", default_value_t = 10)]
    pub grace_secs: u64,
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

/// `crabmatectl` 的子命令名（`crabmate-gen-man` 生成 troff 交叉引用时用）。
///
/// 必须与 [`CtlSubCmd`] 逐项一致——`subcommand_names_match_cli` 会红。这条清单写在这里
/// 而不是生成器里，是因为「新增子命令却忘了进 man 页」不会编译失败，只会留下一条
/// 虚假的 `crabmatectl\-xxx(1)` 交叉引用。
pub const SUBCOMMAND_NAMES: &[&str] = &[
    "up",
    "down",
    "restart",
    "run",
    "ls",
    "status",
    "logs",
    "prune",
    "supervise",
];

/// `crabmatectl` 的根命令。
///
/// **定义在 lib 而不是 `src/bin/crabmatectl.rs`**：`crabmate-gen-man` 是另一个 bin crate
/// 入口，引用不到 bin 内部的类型——定义留在 bin 里，man 页就只能永远手写或用另一份
/// 复制品生成，而那两份迟早会漂移。放在这里，CLI 与手册页来自同一份真源。
#[derive(Parser, Debug, Clone)]
#[command(
    name = "crabmatectl",
    version,
    about = "本地多实例 crabmate serve 进程管理（Linux-only）",
    long_about = None
)]
pub struct CtlCli {
    /// 实例 workspace 路径（`up` / `run` 必填；其余子命令忽略）
    #[arg(long, global = true, value_name = "PATH")]
    pub workspace: Option<String>,

    #[command(subcommand)]
    pub sub: CtlSubCmd,
}

/// 与当前构建一致的 `crabmatectl` 根 `clap::Command`，供 **`crabmate-gen-man`** 生成
/// `man/crabmatectl.1`（troff）。
pub fn ctl_clap_command_for_man_page() -> clap::Command {
    CtlCli::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn man_page_command_is_named_crabmatectl() {
        assert_eq!(ctl_clap_command_for_man_page().get_name(), "crabmatectl");
    }

    /// `restart` 的两个秒数旋钮各自独立：`--grace` 放宽杀旧实例的终止窗口，
    /// `--timeout` 放宽等新实例就绪。两者默认都是 10，映射串了没人看得出来，
    /// 只会得到「按文档放宽了宽限，实际却在等就绪」。
    #[test]
    fn restart_reads_grace_and_timeout_separately() {
        let cli = CtlCli::try_parse_from([
            "crabmatectl", "restart", "a", "--grace", "45", "--timeout", "20",
        ])
        .expect("restart 应接受 --grace 与 --timeout");
        let CtlSubCmd::Restart(c) = cli.sub else {
            panic!("应解析成 restart");
        };
        assert_eq!(c.grace_secs, 45, "`--grace` 是终止宽限");
        assert_eq!(c.timeout_secs, 20, "`--timeout` 是就绪等待");
    }

    /// 默认值同样是契约：不传时 `--grace` 与 `down --timeout` 同义（10s）。
    /// 若默认变成 0，正常关停会直接跳到 SIGKILL。
    #[test]
    fn restart_defaults_grace_and_timeout_to_ten() {
        let cli = CtlCli::try_parse_from(["crabmatectl", "restart", "a"]).expect("restart 只要有实例名");
        let CtlSubCmd::Restart(c) = cli.sub else {
            panic!("应解析成 restart");
        };
        assert_eq!(c.grace_secs, 10);
        assert_eq!(c.timeout_secs, 10);
    }

    /// 新增子命令时若忘了登记 [`SUBCOMMAND_NAMES`]，生成的 man 页会留下虚假交叉引用；
    /// 这条断言把清单和 clap 定义钉在一起。
    #[test]
    fn subcommand_names_match_cli() {
        let cmd = ctl_clap_command_for_man_page();
        let mut actual: Vec<String> = cmd
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        actual.sort();
        let mut expected: Vec<String> = SUBCOMMAND_NAMES.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
