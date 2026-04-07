//! `clap` 派生类型与解析后的中间结构（`ParsedCliArgs` 等在 `parse` 模块组装）。

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

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

/// [`parse_args`](super::parse::parse_args) 的返回值：具名字段替代长元组，便于增删选项与调用方阅读。
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
