use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
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
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("MutexWrite: 日志管道互斥锁已中毒（poisoned）"))?;
        g.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("MutexWrite: 日志管道互斥锁已中毒（poisoned）"))?;
        g.flush()
    }
}

fn open_log_append(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// 初始化 [`log`] + [`env_logger`]。
///
/// - 若已设置环境变量 **`RUST_LOG`**：完全按该变量解析（不强行覆盖默认级别）。
/// - 若未设置 **`RUST_LOG`**：
///   - 指定了 **`log_file`**（`--log <FILE>`）：默认 **`info`**，便于与文件 tail 配套；
///   - **`quiet_cli_default == true`**（非 `--serve` 的 CLI 模式：单次提问、REPL 等）：默认 **`warn`**，不输出 `info`；
///   - 否则（**`serve`**）：默认 **`info`**。
/// - 上述默认过滤器均带 **`tokei=error`**，避免依赖 **`tokei`** 在扫描未知扩展名时以 **`warn`** 刷屏（项目画像 / `code_stats` 等路径）。
///
/// 指定了 `--log` 但无法创建/打开日志文件时返回 [`io::Error`]，由调用方决定如何报告退出码。
pub fn init_logging(log_file: Option<&Path>, quiet_cli_default: bool) -> io::Result<()> {
    use env_logger::{Builder, Env, Target, WriteStyle};

    let env = if std::env::var_os("RUST_LOG").is_some() {
        Env::default()
    } else if log_file.is_some() {
        Env::default().default_filter_or("info,tokei=error")
    } else if quiet_cli_default {
        Env::default().default_filter_or("warn,tokei=error")
    } else {
        Env::default().default_filter_or("info,tokei=error")
    };
    let mut builder = Builder::from_env(env);
    builder.format_target(true);
    builder.format_timestamp_secs();
    match log_file {
        None => {
            builder.target(Target::Stderr);
        }
        Some(path) => {
            let f = open_log_append(path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("无法打开日志文件 {}: {e}", path.display()),
                )
            })?;
            let w = MutexWrite(Mutex::new(StderrAndFile {
                stderr: io::stderr(),
                file: f,
            }));
            builder.target(Target::Pipe(Box::new(w)));
            builder.write_style(WriteStyle::Never);
        }
    }
    builder.init();
    Ok(())
}

/// 从标准输入读取全部内容（直到 EOF）
fn read_stdin_to_string() -> io::Result<String> {
    let mut s = String::new();
    io::stdin().read_to_string(&mut s)?;
    Ok(s)
}

#[inline]
fn is_known_subcommand(s: &str) -> bool {
    matches!(
        s,
        "serve"
            | "repl"
            | "chat"
            | "bench"
            | "config"
            | "doctor"
            | "models"
            | "probe"
            | "mcp"
            | "save-session"
            | "export-session"
            | "tool-replay"
    )
}

/// 若 argv 在 **未写子命令名** 时使用历史平铺 flag（`--serve`、`--query` 等），改写为 `serve` / `chat` / … 形式再交给 clap。
///
/// 已写子命令（如 `crabmate repl` / `crabmate doctor`）或 `-h` / `--help` / `-V` / `--version` 时不改写。
///
/// **`help` 子命令**：`crabmate help` → 根级 `--help`；`crabmate help serve` 等 → 对应子命令 `--help`（否则未写子命令时会被当成 `repl` 的多余参数并报错）。
///
/// 将历史平铺 flag 映射为子命令形式（**契约稳定面**）；与 [`parse_args`] / [`parse_args_from_argv`] 共用。
pub fn normalize_legacy_argv(args: Vec<String>) -> Vec<String> {
    if args.len() <= 1 {
        return args;
    }
    let prog = args[0].clone();
    let rest = &args[1..];
    if rest.first().is_some_and(|s| s == "help") {
        return match rest.len() {
            1 => vec![prog, "--help".into()],
            _ if is_known_subcommand(rest[1].as_str()) => {
                vec![prog, rest[1].clone(), "--help".into()]
            }
            _ => vec![prog, "--help".into()],
        };
    }
    // 任意位置出现显式子命令名（如 `crabmate --workspace /x doctor`）时不再插入默认 `repl`。
    if rest.iter().any(|a| is_known_subcommand(a.as_str())) {
        return args;
    }
    if rest
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "-V" | "--version"))
    {
        return args;
    }

    if rest.iter().any(|a| a == "--dry-run") {
        let mut out = vec![prog, "config".into()];
        for a in rest {
            if a != "--dry-run" {
                out.push(a.clone());
            }
        }
        out.push("--dry-run".into());
        return out;
    }

    if rest.iter().any(|a| a == "--serve") {
        let mut new_rest: Vec<String> = Vec::new();
        let mut i = 0;
        while i < rest.len() {
            if rest[i] == "--serve" {
                i += 1;
                if i < rest.len() && !rest[i].starts_with('-') {
                    i += 1;
                }
                continue;
            }
            new_rest.push(rest[i].clone());
            i += 1;
        }
        let mut out = vec![prog, "serve".into()];
        i = 0;
        while i < rest.len() {
            if rest[i] == "--serve" {
                i += 1;
                if i < rest.len() && !rest[i].starts_with('-') {
                    out.push(rest[i].clone());
                }
                break;
            }
            i += 1;
        }
        out.extend(new_rest);
        return out;
    }

    let has_bench = rest.iter().any(|a| {
        a == "--benchmark"
            || a.starts_with("--benchmark=")
            || a == "--batch"
            || a.starts_with("--batch=")
            || a == "--batch-output"
            || a.starts_with("--batch-output=")
            || a == "--task-timeout"
            || a.starts_with("--task-timeout=")
            || a == "--max-tool-rounds"
            || a.starts_with("--max-tool-rounds=")
            || a == "--resume"
            || a == "--bench-system-prompt"
            || a.starts_with("--bench-system-prompt=")
    });
    if has_bench {
        let mut out = vec![prog, "bench".into()];
        out.extend(rest.iter().cloned());
        return out;
    }

    let has_chat = rest.iter().any(|a| {
        a == "--query"
            || a.starts_with("--query=")
            || a == "--stdin"
            || a == "--output"
            || a.starts_with("--output=")
            || a == "--system-prompt-file"
            || a.starts_with("--system-prompt-file=")
            || a == "--user-prompt-file"
            || a.starts_with("--user-prompt-file=")
            || a == "--messages-json-file"
            || a.starts_with("--messages-json-file=")
            || a == "--message-file"
            || a.starts_with("--message-file=")
            || a == "--yes"
            || a == "--approve-commands"
            || a.starts_with("--approve-commands=")
            || a == "--agent-role"
            || a.starts_with("--agent-role=")
    });
    if has_chat {
        let mut out = vec![prog, "chat".into()];
        out.extend(rest.iter().cloned());
        return out;
    }

    let mut out = vec![prog, "repl".into()];
    out.extend(rest.iter().cloned());
    out
}

/// 各子命令共用的全局选项（须写在子命令之前：`crabmate --config x serve`）。
#[derive(Parser, Debug, Clone, Default)]
pub struct GlobalOpts {
    /// 显式指定配置文件路径（覆盖默认的 config.toml / .agent_demo.toml 搜索）
    #[arg(long, global = true)]
    pub config: Option<String>,

    /// 启动时指定初始工作区路径（覆盖配置中的 run_command_working_dir，仅当前进程生效）
    #[arg(long, global = true)]
    pub workspace: Option<String>,

    /// 禁用所有工具调用，仅作为普通 Chat 使用
    #[arg(long, global = true)]
    pub no_tools: bool,

    /// 新建 REPL / `chat` 会话时使用的命名角色 id（须与配置中 `[[agent_roles]]` 或 `agent_roles.toml` 一致；与 `--system-prompt-file` 互斥时以后者为准）
    #[arg(long = "agent-role", global = true, value_name = "ID")]
    pub agent_role: Option<String>,

    /// 将日志追加写入指定文件（与 `RUST_LOG` 配合）。未设置 `RUST_LOG` 时，指定本选项会启用默认 **info** 级别写入，并同时输出到 stderr
    #[arg(long, global = true, value_name = "FILE")]
    pub log: Option<String>,
}

/// Web 服务
#[derive(Parser, Debug, Clone)]
pub struct ServeCmd {
    /// 监听端口（默认 8080）；与位置参数 `PORT` 二选一，同时给出时以本选项为准
    #[arg(long = "port", value_name = "PORT")]
    pub port: Option<u16>,

    /// 监听端口（位置参数；与 `--port` 二选一）
    #[arg(value_name = "PORT", index = 1)]
    pub port_positional: Option<u16>,

    /// 监听 IP（默认 127.0.0.1）；局域网可设 0.0.0.0
    #[arg(long, value_name = "ADDR")]
    pub host: Option<String>,

    /// 仅提供后端 API，不挂载前端静态页面
    #[arg(long, alias = "cli-only")]
    pub no_web: bool,
}

/// 交互式 REPL（无子命令时默认进入 REPL）
#[derive(Parser, Debug, Clone, Default)]
pub struct ReplCmd {
    /// 关闭流式输出，等待完整回答后一次性打印
    #[arg(long)]
    pub no_stream: bool,
}

/// `chat` 子命令解析结果（非 `chat` 子命令时见 [`ChatCliArgs::default`]）。
#[derive(Debug, Clone, Default)]
pub struct ChatCliArgs {
    /// `--query` 或 `--stdin` 读入的用户正文（`--user-prompt-file` 时在运行时读文件）
    pub inline_user_text: Option<String>,
    pub user_prompt_file: Option<String>,
    pub system_prompt_file: Option<String>,
    pub messages_json_file: Option<String>,
    pub message_file: Option<String>,
    pub output: Option<String>,
    pub no_stream: bool,
    pub yes_run_command: bool,
    pub approve_commands: Option<String>,
}

impl ChatCliArgs {
    /// 是否应走 `chat` 流程（`repl`/`serve` 等路径下为默认空，恒为 false）。
    pub fn wants_chat(&self) -> bool {
        self.message_file.is_some()
            || self.messages_json_file.is_some()
            || self.user_prompt_file.is_some()
            || self.inline_user_text.is_some()
    }
}

/// 单次或批处理提问（脚本 / CI）
#[derive(Parser, Debug, Clone)]
#[command(group(
    clap::ArgGroup::new("chat_user_text_exclusive")
        .args(["query", "stdin", "user_prompt_file"])
        .multiple(false),
))]
#[command(group(
    clap::ArgGroup::new("chat_one_source")
        .required(true)
        .args([
            "query",
            "stdin",
            "user_prompt_file",
            "messages_json_file",
            "message_file",
        ]),
))]
#[command(
    after_long_help = "进程退出码与 `--output json` 稳定 JSON 行（`crabmate_chat_cli_result` v=1）见仓库 **docs/CLI_CONTRACT.md**；SSE 流错误码（如 INTERNAL_ERROR）见 **docs/SSE_PROTOCOL.md**。"
)]
pub struct ChatCmd {
    /// 直接在参数中给出用户消息
    #[arg(long, value_name = "TEXT")]
    pub query: Option<String>,

    /// 从标准输入读取用户消息（直到 EOF）
    #[arg(long)]
    pub stdin: bool,

    /// 从文件读取用户消息（与 `--query`/`--stdin` 三选一）
    #[arg(long, value_name = "FILE")]
    pub user_prompt_file: Option<String>,

    /// 覆盖本轮 system 提示词（与配置合并语义：仅替换 seed 中的 system，不含工作区注入）
    #[arg(long, value_name = "FILE")]
    pub system_prompt_file: Option<String>,

    /// 单轮完整 `messages` JSON：顶层数组，或 `{"messages":[...]}`（OpenAI 兼容字段）
    #[arg(long, value_name = "FILE")]
    pub messages_json_file: Option<String>,

    /// 多轮批跑 JSONL：每行 `{"user":"…"}` 追加用户消息后跑一轮，或 `{"messages":[...]}` 整表替换后跑一轮
    #[arg(long = "message-file", value_name = "FILE")]
    pub message_file: Option<String>,

    /// plain（默认）或 json：每轮结束后 stdout 一行 JSON（`type=crabmate_chat_cli_result`，见 docs/CLI_CONTRACT.md）
    #[arg(long, value_name = "MODE")]
    pub output: Option<String>,

    #[arg(long)]
    pub no_stream: bool,

    /// 自动批准所有非白名单 `run_command`（**仅可信环境**）
    #[arg(long)]
    pub yes: bool,

    /// 逗号分隔命令名，与配置白名单合并，匹配者不经终端确认即可 `run_command`
    #[arg(long, value_name = "NAMES")]
    pub approve_commands: Option<String>,
}

/// 批量测评
#[derive(Parser, Debug, Clone)]
pub struct BenchCmd {
    #[arg(long, value_name = "TYPE")]
    pub benchmark: Option<String>,

    #[arg(long, value_name = "FILE")]
    pub batch: Option<String>,

    #[arg(long, value_name = "FILE")]
    pub batch_output: Option<String>,

    #[arg(long, value_name = "SECS", default_value = "300")]
    pub task_timeout: u64,

    #[arg(long, value_name = "N", default_value = "0")]
    pub max_tool_rounds: usize,

    #[arg(long)]
    pub resume: bool,

    #[arg(long, value_name = "FILE")]
    pub bench_system_prompt: Option<String>,
}

/// MCP 运维子命令（只读列出本进程内 stdio 会话缓存）
#[derive(Parser, Debug, Clone)]
pub struct McpCmd {
    #[command(subcommand)]
    pub sub: McpSubCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum McpSubCmd {
    /// 列出与当前配置指纹一致的已缓存 MCP 会话及合并后的 OpenAI 工具名
    List(McpListCmd),
}

#[derive(Parser, Debug, Clone)]
pub struct McpListCmd {
    /// 按配置尝试建立一次 stdio 连接并刷新进程内缓存（排障用；会启动 mcp_command 子进程）
    #[arg(long)]
    pub probe: bool,
}

/// 配置检查（不发起对话）
#[derive(Parser, Debug, Clone, Default)]
pub struct ConfigCmd {
    /// 可选；与不带本参数相同，均为一次配置检查后退出（供脚本显式标注）
    #[arg(long)]
    pub dry_run: bool,
}

/// 将会话 JSON 导出为与 Web 一致的 `chat_export_*.json` / `.md`（**不要**求 `API_KEY`）
#[derive(Parser, Debug, Clone)]
pub struct SaveSessionCmd {
    /// 导出格式（默认两者皆写）
    #[arg(long, value_enum, default_value_t = SaveSessionFormat::Both)]
    pub format: SaveSessionFormat,

    /// 会话文件（默认：`<workspace>/.crabmate/tui_session.json`）
    #[arg(long, value_name = "FILE")]
    pub session_file: Option<String>,
}

/// `save-session --format` 取值
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SaveSessionFormat {
    /// 仅 JSON（`ChatSessionFile` v1，与前端导出同形）
    Json,
    /// 仅 Markdown
    Markdown,
    /// JSON + Markdown 各一份
    #[default]
    Both,
}

/// 解析后的 `save-session` 参数（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub struct SaveSessionCli {
    pub format: SaveSessionFormat,
    pub session_file: Option<String>,
}

/// `tool-replay` 子命令解析结果（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub enum ToolReplayCli {
    /// 从会话 JSON 提取工具调用序列为 fixture
    Export {
        session_file: Option<String>,
        output: Option<String>,
        note: Option<String>,
    },
    /// 按 fixture 重放工具（不调用大模型）
    Run {
        fixture: String,
        compare_recorded: bool,
    },
}

/// 从 `chat_export` / `tui_session.json` 提取工具步骤为可重放 fixture，或重放 fixture（**不要**求 `API_KEY`）
#[derive(Parser, Debug, Clone)]
pub struct ToolReplayCmd {
    #[command(subcommand)]
    pub sub: ToolReplaySubCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ToolReplaySubCmd {
    /// 写入 `<workspace>/.crabmate/exports/tool_replay_*.json`（或 `--output`）
    Export(ToolReplayExportCmd),
    /// 在当前工作区按配置执行 fixture 中每条工具（与对话路径相同 `run_tool`）
    Run(ToolReplayRunCmd),
}

#[derive(Parser, Debug, Clone)]
pub struct ToolReplayExportCmd {
    /// 会话 JSON（默认：`<workspace>/.crabmate/tui_session.json`；可与 `save-session` 导出文件相同）
    #[arg(long, value_name = "FILE")]
    pub session_file: Option<String>,

    /// 输出路径（默认：exports 目录下带时间戳文件名）
    #[arg(long, value_name = "FILE")]
    pub output: Option<String>,

    /// 写入 fixture 顶层的可选说明（供人读）
    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct ToolReplayRunCmd {
    /// `tool-replay export` 生成的 JSON
    #[arg(long, value_name = "FILE")]
    pub fixture: String,

    /// 若步骤含 `recorded_output`，与本次执行结果做字符串全等比较；有不一致则退出码 6
    #[arg(long)]
    pub compare_recorded: bool,
}

/// `parse_args` 扩展槽：非默认 CLI 流程（doctor / models / probe）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtraCliCommand {
    #[default]
    None,
    Doctor,
    Models,
    Probe,
    /// `mcp list`（`probe` 见子命令 `--probe`）
    McpList {
        probe: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// 启动 Web UI + HTTP API（默认端口 8080）
    Serve(ServeCmd),
    /// 交互式终端对话（默认子命令）
    Repl(ReplCmd),
    /// 单次提问后退出（脚本/管道）
    Chat(ChatCmd),
    /// 批量 benchmark 测评（JSONL）
    Bench(BenchCmd),
    /// 配置与自检（如 dry-run）
    Config(ConfigCmd),
    /// 一页本地诊断（Rust/npm/前端路径、白名单条数等；人读，脱敏；**不要**求 API_KEY）
    Doctor,
    /// 列出兼容网关 `GET …/models` 的模型 id（`llm_http_auth_mode=bearer` 时需 API_KEY；部分网关无此端点）
    Models,
    /// 探测 api_base 上 models 端点连通性与 HTTP 状态（`llm_http_auth_mode=bearer` 时需 API_KEY）
    Probe,
    /// 从会话文件导出 JSON/Markdown 到工作区 `.crabmate/exports/`（与 Web 导出约定一致；**不要**求 API_KEY）
    #[command(name = "save-session", visible_alias = "export-session")]
    SaveSession(SaveSessionCmd),
    /// 工具调用时间线导出与重放（fixture / 回归；**不要**求 `API_KEY`）
    #[command(name = "tool-replay")]
    ToolReplay(ToolReplayCmd),
    /// MCP stdio 客户端运维：列出本进程内已缓存会话（**不要**求 API_KEY）
    Mcp(McpCmd),
}

#[derive(Parser, Debug)]
#[command(
    name = "crabmate",
    version,
    about = "基于 OpenAI 兼容 chat/completions 的 Rust AI Agent（DeepSeek / MiniMax / Ollama 等），支持工具调用、Web 界面与 CLI",
    after_long_help = "CLI 退出码、`chat --output json` 行协议与 SSE 错误码交叉引用：**docs/CLI_CONTRACT.md**、**docs/SSE_PROTOCOL.md**。子命令详情：**docs/CLI.md**。"
)]
pub struct RootCli {
    #[command(flatten)]
    pub global: GlobalOpts,

    /// 未指定时进入 `repl`
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// 与当前构建一致的根级 `clap::Command`，供 **`crabmate-gen-man`** 生成 `man/crabmate.1`（troff）。
pub fn root_clap_command_for_man_page() -> clap::Command {
    RootCli::command()
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

/// [`parse_args`] 的返回值：具名字段替代长元组，便于增删选项与调用方阅读。
#[derive(Debug, Clone)]
pub struct ParsedCliArgs {
    pub config_path: Option<String>,
    /// 全局 `--agent-role`：REPL / `chat` 新建会话首条 system 用（配置须含该 id）
    pub agent_role_cli: Option<String>,
    pub chat_cli: ChatCliArgs,
    pub serve_port: Option<u16>,
    /// `serve` 时使用；来自 `serve --host`、`AGENT_HTTP_HOST` 或默认 `127.0.0.1`。
    pub http_bind_host: String,
    pub workspace_cli: Option<String>,
    pub no_tools: bool,
    pub no_web: bool,
    pub dry_run: bool,
    pub no_stream: bool,
    pub log_file: Option<String>,
    pub bench_args: BenchmarkCliArgs,
    pub extra_cli: ExtraCliCommand,
    /// `Some` 时执行导出后退出（与 `doctor` 一样不要求 API_KEY）
    pub save_session: Option<SaveSessionCli>,
    /// `Some` 时执行工具重放子命令后退出（不要求 API_KEY）
    pub tool_replay: Option<ToolReplayCli>,
}

fn parse_output_mode(raw: Option<String>) -> Option<String> {
    raw.as_ref().and_then(|m| {
        let m = m.to_ascii_lowercase();
        if m == "json" || m == "plain" {
            Some(m)
        } else {
            None
        }
    })
}

/// 解析命令行：支持 **`serve` / `repl` / `chat` / `bench` / `config` / `doctor` / `models` / `probe` / `mcp` / `save-session`**（兼容别名 **`export-session`**）、**`tool-replay`** 子命令，**`help`**（同 `--help` 或 `help <子命令>`），并兼容未写子命令时的历史平铺 flag（`--serve`、`--query` 等）。
///
/// `chat --stdin` 时若读取标准输入失败则返回 [`io::Error`]。
///
/// 非法 CLI：打印 clap 说明后以 **非零** 码退出进程（与历史 `parse_from` 行为一致）；**不会**向调用方返回 `Err`。
pub fn parse_args() -> io::Result<ParsedCliArgs> {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized).unwrap_or_else(|e| e.exit());
    build_parsed_cli_args(root, None)
}

/// 使用给定 **`argv`**（首元素为程序名）解析 CLI，供契约/集成测试；生产请用 [`parse_args`]。
///
/// - **`stdin_fixture`**：当参数含 `chat --stdin` 时，使用该字符串代替读取真实 stdin（避免测试挂起）。
/// - 非法参数：返回 [`io::Error`]（**不**退出进程），便于断言。
pub fn parse_args_from_argv(
    raw: Vec<String>,
    stdin_fixture: Option<String>,
) -> io::Result<ParsedCliArgs> {
    let normalized = normalize_legacy_argv(raw);
    let root = RootCli::try_parse_from(normalized)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    build_parsed_cli_args(root, stdin_fixture)
}

fn build_parsed_cli_args(
    root: RootCli,
    stdin_fixture: Option<String>,
) -> io::Result<ParsedCliArgs> {
    let GlobalOpts {
        config,
        workspace,
        no_tools,
        log,
        agent_role,
    } = root.global;
    let agent_role_cli = agent_role
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let log_path = log
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let http_bind_host = |host_opt: Option<String>| {
        host_opt
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("AGENT_HTTP_HOST")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "127.0.0.1".to_string())
    };

    Ok(match root.command {
        None => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Serve(s)) => {
            let port = s.port.or(s.port_positional).or(Some(8080));
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: port,
                http_bind_host: http_bind_host(s.host),
                workspace_cli: workspace,
                no_tools,
                no_web: s.no_web,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: None,
            }
        }
        Some(Commands::Repl(r)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: r.no_stream,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Chat(c)) => {
            let inline_user_text = if c.user_prompt_file.is_some() {
                None
            } else if c.stdin {
                match stdin_fixture.as_ref() {
                    Some(s) => Some(s.clone()),
                    None => Some(read_stdin_to_string()?),
                }
            } else {
                c.query.clone()
            };
            let chat_output = parse_output_mode(c.output);
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs {
                    inline_user_text,
                    user_prompt_file: c.user_prompt_file,
                    system_prompt_file: c.system_prompt_file,
                    messages_json_file: c.messages_json_file,
                    message_file: c.message_file,
                    output: chat_output,
                    no_stream: c.no_stream,
                    yes_run_command: c.yes,
                    approve_commands: c.approve_commands,
                },
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools,
                no_web: false,
                dry_run: false,
                no_stream: c.no_stream,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: None,
            }
        }
        Some(Commands::Bench(b)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs {
                benchmark: b.benchmark,
                batch: b.batch,
                batch_output: b.batch_output,
                task_timeout: b.task_timeout,
                max_tool_rounds: b.max_tool_rounds,
                resume: b.resume,
                system_prompt_file: b.bench_system_prompt,
            },
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        // `config` 子命令恒走配置检查并退出，与是否写 `--dry-run` 无关（`--dry-run` 保留为显式别名）。
        Some(Commands::Config(_c)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: true,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Doctor) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Doctor,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Models) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Models,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::Probe) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::Probe,
            save_session: None,
            tool_replay: None,
        },
        Some(Commands::SaveSession(e)) => ParsedCliArgs {
            config_path: config,
            agent_role_cli: agent_role_cli.clone(),
            chat_cli: ChatCliArgs::default(),
            serve_port: None,
            http_bind_host: http_bind_host(None),
            workspace_cli: workspace,
            no_tools,
            no_web: false,
            dry_run: false,
            no_stream: false,
            log_file: log_path,
            bench_args: BenchmarkCliArgs::default(),
            extra_cli: ExtraCliCommand::None,
            save_session: Some(SaveSessionCli {
                format: e.format,
                session_file: e.session_file,
            }),
            tool_replay: None,
        },
        Some(Commands::ToolReplay(tr)) => {
            let tr_cli = match tr.sub {
                ToolReplaySubCmd::Export(e) => ToolReplayCli::Export {
                    session_file: e.session_file,
                    output: e.output,
                    note: e.note,
                },
                ToolReplaySubCmd::Run(r) => ToolReplayCli::Run {
                    fixture: r.fixture,
                    compare_recorded: r.compare_recorded,
                },
            };
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools,
                no_web: false,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::None,
                save_session: None,
                tool_replay: Some(tr_cli),
            }
        }
        Some(Commands::Mcp(m)) => {
            let probe = match m.sub {
                McpSubCmd::List(l) => l.probe,
            };
            ParsedCliArgs {
                config_path: config,
                agent_role_cli: agent_role_cli.clone(),
                chat_cli: ChatCliArgs::default(),
                serve_port: None,
                http_bind_host: http_bind_host(None),
                workspace_cli: workspace,
                no_tools,
                no_web: false,
                dry_run: false,
                no_stream: false,
                log_file: log_path,
                bench_args: BenchmarkCliArgs::default(),
                extra_cli: ExtraCliCommand::McpList { probe },
                save_session: None,
                tool_replay: None,
            }
        }
    })
}

#[cfg(test)]
mod legacy_argv_tests {
    use super::{RootCli, normalize_legacy_argv};
    use clap::Parser;

    fn norm(args: &[&str]) -> Vec<String> {
        normalize_legacy_argv(args.iter().map(|s| (*s).to_string()).collect())
    }

    #[test]
    fn explicit_subcommand_unchanged() {
        let v = norm(&["crabmate", "serve", "3000"]);
        assert_eq!(v, vec!["crabmate", "serve", "3000"]);
    }

    #[test]
    fn help_save_session_routes_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "save-session"]);
        assert_eq!(v, vec!["crabmate", "save-session", "--help"]);
    }

    #[test]
    fn help_export_session_alias_routes_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "export-session"]);
        assert_eq!(v, vec!["crabmate", "export-session", "--help"]);
    }

    #[test]
    fn explicit_doctor_subcommand_not_prefixed_with_repl() {
        let v = norm(&["crabmate", "doctor"]);
        assert_eq!(v, vec!["crabmate", "doctor"]);
    }

    #[test]
    fn try_parse_root_doctor_subcommand() {
        let r = RootCli::try_parse_from(vec!["crabmate".to_string(), "doctor".to_string()]);
        assert!(r.is_ok(), "{:?}", r.as_ref().err());
        assert!(matches!(r.unwrap().command, Some(super::Commands::Doctor)));
    }

    #[test]
    fn parse_args_from_argv_doctor_matches_extra_cli() {
        let p =
            super::parse_args_from_argv(vec!["crabmate".to_string(), "doctor".to_string()], None)
                .unwrap();
        assert_eq!(p.extra_cli, super::ExtraCliCommand::Doctor);
    }

    #[test]
    fn legacy_serve_with_port() {
        let v = norm(&["crabmate", "--serve", "3000", "--no-web"]);
        assert_eq!(v, vec!["crabmate", "serve", "3000", "--no-web"]);
    }

    #[test]
    fn legacy_serve_default_port() {
        let v = norm(&["crabmate", "--serve"]);
        assert_eq!(v, vec!["crabmate", "serve"]);
    }

    #[test]
    fn legacy_serve_then_host() {
        let v = norm(&["crabmate", "--serve", "--host", "0.0.0.0"]);
        assert_eq!(v, vec!["crabmate", "serve", "--host", "0.0.0.0"]);
    }

    #[test]
    fn legacy_repl_implicit() {
        let v = norm(&["crabmate", "--no-stream"]);
        assert_eq!(v, vec!["crabmate", "repl", "--no-stream"]);
    }

    #[test]
    fn legacy_chat() {
        let v = norm(&["crabmate", "--query", "hi"]);
        assert_eq!(v, vec!["crabmate", "chat", "--query", "hi"]);
    }

    #[test]
    fn legacy_chat_message_file_maps() {
        let v = norm(&["crabmate", "--message-file", "cases.jsonl"]);
        assert_eq!(v, vec!["crabmate", "chat", "--message-file", "cases.jsonl"]);
    }

    #[test]
    fn legacy_config_dry_run() {
        let v = norm(&["crabmate", "--dry-run"]);
        assert_eq!(v, vec!["crabmate", "config", "--dry-run"]);
    }

    #[test]
    fn help_not_wrapped() {
        let v = norm(&["crabmate", "--help"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn help_subcommand_maps_to_root_help() {
        let v = norm(&["crabmate", "help"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn help_known_subcommand_maps_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "serve"]);
        assert_eq!(v, vec!["crabmate", "serve", "--help"]);
    }

    #[test]
    fn help_doctor_maps_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "doctor"]);
        assert_eq!(v, vec!["crabmate", "doctor", "--help"]);
    }

    #[test]
    fn help_unknown_second_token_falls_back_to_root_help() {
        let v = norm(&["crabmate", "help", "nope"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }
}
