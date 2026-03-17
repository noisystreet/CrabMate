use clap::Parser;
use std::io::{self, Read};

/// 初始化日志订阅器（使用 RUST_LOG 环境变量控制级别）
pub fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
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
}

/// 兼容原有调用处的解析函数，基于 clap::Parser 实现
pub fn parse_args() -> (
    Option<String>,
    Option<String>,
    Option<u16>,
    Option<String>,
    Option<String>,
    bool, // no_tools
    bool, // no_web
    bool, // dry_run
    bool, // no_stream
) {
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

    (
        cli.config,
        single_shot,
        serve_port,
        cli.workspace,
        output_mode,
        cli.no_tools,
        cli.no_web,
        cli.dry_run,
        cli.no_stream,
    )
}

