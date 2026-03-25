use clap::Parser;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;

/// 同时写 stderr 与日志文件（单条日志一份内容；关闭 ANSI 便于 `tail`）。
struct StderrAndFile {
    stderr: io::Stderr,
    file: std::fs::File,
}

impl Write for StderrAndFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stderr.write_all(buf)?;
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()?;
        self.file.flush()
    }
}

/// 供 `env_logger::Target::Pipe` 使用的 `Write`（内部 `Mutex`）。
struct MutexWrite<W: Write + Send>(Mutex<W>);

impl<W: Write + Send> Write for MutexWrite<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

fn open_log_append(path: &Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|e| {
            eprintln!("无法打开日志文件 {}: {}", path.display(), e);
            std::process::exit(1);
        })
}

/// 初始化 [`log`] + [`env_logger`]。
///
/// - 若已设置环境变量 **`RUST_LOG`**：完全按该变量解析（不强行覆盖默认级别）。
/// - 若未设置 **`RUST_LOG`**：
///   - 指定了 **`log_file`**（`--log <FILE>`）：默认 **`info`**，便于与文件 tail 配套；
///   - **`quiet_cli_default == true`**（非 `--serve` 的 CLI 模式：单次提问、REPL、TUI 等）：默认 **`warn`**，不输出 `info`；
///   - 否则（**`--serve`**）：默认 **`info`**。
///
/// `suppress_stdio_logs`：为 **TUI 全屏** 设为 `true`，避免日志行破坏界面；若同时传入 `log_file`，则只写文件。
pub fn init_logging(suppress_stdio_logs: bool, log_file: Option<&Path>, quiet_cli_default: bool) {
    use env_logger::{Builder, Env, Target, WriteStyle};

    let env = if std::env::var_os("RUST_LOG").is_some() {
        Env::default()
    } else if log_file.is_some() {
        Env::default().default_filter_or("info")
    } else if quiet_cli_default {
        Env::default().default_filter_or("warn")
    } else {
        Env::default().default_filter_or("info")
    };
    let mut builder = Builder::from_env(env);
    builder.format_target(true);
    builder.format_timestamp_secs();
    match (suppress_stdio_logs, log_file) {
        (true, None) => {
            builder.target(Target::Pipe(Box::new(MutexWrite(Mutex::new(
                std::io::sink(),
            )))));
            builder.write_style(WriteStyle::Never);
        }
        (false, None) => {
            builder.target(Target::Stderr);
        }
        (true, Some(path)) => {
            let f = open_log_append(path);
            builder.target(Target::Pipe(Box::new(MutexWrite(Mutex::new(f)))));
            builder.write_style(WriteStyle::Never);
        }
        (false, Some(path)) => {
            let f = open_log_append(path);
            let w = MutexWrite(Mutex::new(StderrAndFile {
                stderr: io::stderr(),
                file: f,
            }));
            builder.target(Target::Pipe(Box::new(w)));
            builder.write_style(WriteStyle::Never);
        }
    }
    builder.init();
}

/// 从标准输入读取全部内容（直到 EOF）
fn read_stdin_to_string() -> String {
    let mut s = String::new();
    let _ = io::stdin().read_to_string(&mut s);
    s
}

/// 命令行参数定义（使用 clap 解析）
#[derive(Parser, Debug)]
#[command(
    name = "CrabMate",
    version,
    about = "基于 DeepSeek API 的简易 Agent，支持工具调用、Web 界面与命令行交互"
)]
pub struct Cli {
    /// 显式指定配置文件路径（覆盖默认的 config.toml / .agent_demo.toml 搜索）
    #[arg(long)]
    pub config: Option<String>,

    /// 以 Web 服务启动，端口可选（未指定时默认 8080）
    #[arg(long, num_args = 0..=1, value_name = "PORT")]
    pub serve: Option<Option<u16>>,

    /// Web 监听 IP（仅 `--serve` 时生效）。默认 127.0.0.1；需局域网/容器对外暴露时可传 0.0.0.0
    #[arg(long, value_name = "ADDR")]
    pub host: Option<String>,

    /// 单次提问：直接在命令行参数中给出问题
    #[arg(long, value_name = "QUESTION")]
    pub query: Option<String>,

    /// 从标准输入读取问题（多行直到 EOF）
    #[arg(long)]
    pub stdin: bool,

    /// 启动时指定初始工作区路径（覆盖配置中的 run_command_working_dir，仅当前进程生效）
    #[arg(long)]
    pub workspace: Option<String>,

    /// 仅对 --query / --stdin 生效；plain 为默认，json 会在末尾额外输出一行 JSON 结果
    #[arg(long, value_name = "MODE")]
    pub output: Option<String>,

    /// 禁用所有工具调用，仅作为普通 Chat 使用
    #[arg(long)]
    pub no_tools: bool,

    /// 仅提供后端 API，不挂载前端静态页面（适合作为纯后端服务）
    #[arg(long, alias = "cli-only")]
    pub no_web: bool,

    /// 仅检查配置、API_KEY 与前端静态目录是否存在，然后退出（用于 CI 自检）
    #[arg(long)]
    pub dry_run: bool,

    /// 在命令行模式下关闭流式输出，等待完整回答后一次性打印
    #[arg(long)]
    pub no_stream: bool,

    /// 启动完整终端 UI（TUI，左侧对话，右侧工作区/任务/日程）
    #[arg(long)]
    pub tui: bool,

    /// 将日志追加写入指定文件（与 `RUST_LOG` 配合）。未设置 `RUST_LOG` 时，指定本选项会启用默认 **info** 级别写入。CLI 下可同时输出到 stderr；TUI 下默认不写 stderr，可用本选项后台 `tail -f` 查看
    #[arg(long, value_name = "FILE")]
    pub log: Option<String>,

    // ---- Benchmark 批量测评 ----
    /// 批量测评模式：指定 benchmark 类型（swe_bench / gaia / human_eval / generic）
    #[arg(long, value_name = "TYPE")]
    pub benchmark: Option<String>,

    /// 输入 JSONL 文件路径（每行一条 benchmark 任务）
    #[arg(long, value_name = "FILE")]
    pub batch: Option<String>,

    /// 输出 JSONL 文件路径（逐条追加写入结果）
    #[arg(long, value_name = "FILE")]
    pub batch_output: Option<String>,

    /// 每条任务的全局超时（秒，0 = 不限制，默认 300）
    #[arg(long, value_name = "SECS", default_value = "300")]
    pub task_timeout: u64,

    /// 每条任务最大 agent 工具调用轮次（0 = 不限制）
    #[arg(long, value_name = "N", default_value = "0")]
    pub max_tool_rounds: usize,

    /// 续跑模式：跳过输出文件中已有结果的 instance_id
    #[arg(long)]
    pub resume: bool,

    /// 批量测评时覆盖 system prompt（文件路径）
    #[arg(long, value_name = "FILE")]
    pub bench_system_prompt: Option<String>,
}

/// Benchmark 批量测评相关的 CLI 参数。
#[derive(Debug, Clone, Default)]
pub struct BenchmarkCliArgs {
    pub benchmark: Option<String>,
    pub batch: Option<String>,
    pub batch_output: Option<String>,
    pub task_timeout: u64,
    pub max_tool_rounds: usize,
    pub resume: bool,
    pub system_prompt_file: Option<String>,
}

/// `parse_args` 的返回值：配置路径、单次提问、serve 端口、Web 绑定地址、输出模式、工作区 CLI、各类布尔开关、日志文件路径、benchmark 参数。
pub type ParsedCliArgs = (
    Option<String>,
    Option<String>,
    Option<u16>,
    String, // http_bind_host（`--serve` 时使用；由 `--host` 或 AGENT_HTTP_HOST 或默认 127.0.0.1）
    Option<String>,
    Option<String>,
    bool,           // no_tools
    bool,           // no_web
    bool,           // dry_run
    bool,           // no_stream
    bool,           // tui
    Option<String>, // log file path
    BenchmarkCliArgs,
);

/// 兼容原有调用处的解析函数，基于 clap::Parser 实现
pub fn parse_args() -> ParsedCliArgs {
    let cli = Cli::parse();

    // serve: None 表示未传；Some(None) 表示传了但没给端口 => 默认 8080；Some(Some(p)) 为指定端口
    let serve_port = match cli.serve {
        None => None,
        Some(None) => Some(8080),
        Some(Some(p)) => Some(p),
    };

    // 单次问题：来自 --query 或 --stdin
    let single_shot = if let Some(q) = cli.query {
        Some(q)
    } else if cli.stdin {
        Some(read_stdin_to_string())
    } else {
        None
    };

    let output_mode = cli.output.as_ref().and_then(|m| {
        let m = m.to_ascii_lowercase();
        if m == "json" || m == "plain" {
            Some(m)
        } else {
            None
        }
    });

    let http_bind_host = cli
        .host
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("AGENT_HTTP_HOST")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let log_path = cli
        .log
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let bench_args = BenchmarkCliArgs {
        benchmark: cli.benchmark,
        batch: cli.batch,
        batch_output: cli.batch_output,
        task_timeout: cli.task_timeout,
        max_tool_rounds: cli.max_tool_rounds,
        resume: cli.resume,
        system_prompt_file: cli.bench_system_prompt,
    };

    (
        cli.config,
        single_shot,
        serve_port,
        http_bind_host,
        cli.workspace,
        output_mode,
        cli.no_tools,
        cli.no_web,
        cli.dry_run,
        cli.no_stream,
        cli.tui,
        log_path,
        bench_args,
    )
}
