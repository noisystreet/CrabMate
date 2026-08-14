//! `clap` 派生类型与解析后的中间结构（`ParsedCliArgs` 等在 `parse` 模块组装）。

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

/// 各子命令共用的全局选项（须写在子命令之前：`crabmate --config x serve`）。
#[derive(Parser, Debug, Clone, Default)]
pub struct GlobalOpts {
    /// 显式指定配置文件路径（覆盖默认的 cwd / XDG `config.toml` 搜索）
    #[arg(long, global = true)]
    pub config: Option<String>,

    /// 启动时指定初始工作区路径（覆盖配置中的 run_command_working_dir，仅当前进程生效）
    #[arg(long, global = true)]
    pub workspace: Option<String>,

    /// 禁用所有工具调用，仅作为普通 Chat 使用
    #[arg(long, global = true)]
    pub no_tools: bool,

    /// 模型上下文窗口 token 上限（输入+输出），覆盖配置 `llm_context_tokens` / `CM_LLM_CONTEXT_TOKENS`；省略则使用配置文件；`0` 视为不覆盖
    #[arg(long = "llm-context-tokens", global = true, value_name = "N")]
    pub llm_context_tokens: Option<u32>,

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

    /// 显式挂载业务 UI 静态资源（Client `frontend/dist` / `CM_WEB_STATIC_DIR`）。
    /// **默认不挂**（纯 API）；同机托管 SPA 须传本旗标。
    #[arg(long = "with-web", alias = "web")]
    pub with_web: bool,

    /// 监听成功后向 stdout 输出一行 `{"event":"web_ready",...}` JSON；壳不再依赖，仅脚本/工具。
    /// 旗标名 `--desktop-ready-json` 已弃用命名，请优先使用可见别名 `--web-ready-json`。
    #[arg(long = "desktop-ready-json", visible_alias = "web-ready-json")]
    pub desktop_ready_json: bool,
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

    #[arg(long, value_name = "N", default_value = "1")]
    pub samples: usize,

    #[arg(long)]
    pub resume: bool,

    #[arg(long, value_name = "FILE")]
    pub bench_system_prompt: Option<String>,
}

/// Web API Bearer：写入/查询/清除系统钥匙串（与 Web「Web API 共享密钥」同源；**不要**求 `API_KEY`）
#[derive(Parser, Debug, Clone)]
#[command(name = "web-bearer")]
pub struct WebBearerCmd {
    #[command(subcommand)]
    pub sub: WebBearerSubCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WebBearerSubCmd {
    /// 是否已在系统钥匙串设置 Web API Bearer（不打印明文）
    Status,
    /// 写入系统钥匙串（与 `CM_WEB_API_BEARER_TOKEN` / TOML 为空时 `serve` 回退同源）
    Set(WebBearerSetCmd),
    /// 清除系统钥匙串中的 Web API Bearer
    Clear,
}

#[derive(Parser, Debug, Clone)]
pub struct WebBearerSetCmd {
    /// 共享密钥（会出现在 shell 历史 / `ps`；推荐改用 **`--stdin`** / **`--from-env`**，或无参数时交互隐藏输入）
    #[arg(value_name = "TOKEN")]
    pub token: Option<String>,

    /// 从标准输入读取密钥（一行；适合 `printf '%s' "$TOKEN" | crabmate web-bearer set --stdin`）
    #[arg(long)]
    pub stdin: bool,

    /// 从环境变量 **`CM_WEB_API_BEARER_TOKEN`** 读取并写入钥匙串（不经 argv）
    #[arg(long)]
    pub from_env: bool,
}

/// `web-bearer` 解析结果（供 `runtime` 执行；**不要**求 `API_KEY`）
#[derive(Debug, Clone)]
pub enum WebBearerCli {
    Status,
    /// `token` 仅在解析到位置参数时有值；`--stdin` / `--from-env` / 交互提示在运行时解析
    Set {
        token: Option<String>,
        stdin: bool,
        from_env: bool,
    },
    Clear,
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
    /// 在本进程 stdin/stdout 上运行 MCP server，暴露内置工具（**不要**求 API_KEY；无传输鉴权）
    Serve(McpServeCmd),
}

#[derive(Parser, Debug, Clone)]
pub struct McpListCmd {
    /// 按配置尝试建立一次 stdio 连接并刷新进程内缓存（排障用；会启动 mcp_command 子进程）
    #[arg(long)]
    pub probe: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct McpServeCmd {
    /// 不向客户端列出任何工具（仍接受 `tools/call`，将返回未知工具）
    #[arg(long)]
    pub no_tools: bool,
    /// TCP 端口（默认 0 表示 stdio 模式）。设置后监听 TCP 连接而非 stdio。
    #[arg(long, default_value_t = 0)]
    pub port: u16,
}

/// 工作流作者层：YAML / Markdown → `workflow.nodes` 编译与校验（**不要**求 API_KEY）
#[derive(Parser, Debug, Clone)]
pub struct WorkflowCmd {
    #[command(subcommand)]
    pub sub: WorkflowSubCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkflowSubCmd {
    /// 编译作者层 YAML 为 `workflow_execute` JSON（stdout）
    Compile(WorkflowFileCmd),
    /// 编译并校验 DAG（拓扑层、工具参数 schema）
    Validate(WorkflowFileCmd),
    /// 编译并执行 DAG（与 Agent 调 `workflow_execute` + `workflow_file` 相同；**不要**求 API_KEY）
    Run(WorkflowFileCmd),
}

#[derive(Parser, Debug, Clone)]
pub struct WorkflowFileCmd {
    /// `.yaml` / `.yml` 或含 `` ```crabmate-workflow `` 的 `.md`
    pub file: String,
    /// `validate` 时以 JSON 打印层与节点摘要
    #[arg(long)]
    pub json: bool,
}

/// `workflow validate|compile` 解析结果
#[derive(Debug, Clone)]
pub struct WorkflowFileCli {
    pub file: String,
    pub json: bool,
}

/// 动态工具模板与校验
#[derive(Parser, Debug, Clone)]
pub struct PluginCmd {
    #[command(subcommand)]
    pub sub: PluginSubCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PluginSubCmd {
    /// 生成 `plugins/*.json` 动态工具模板（名称须以 `dyn__` 开头）
    Init(PluginInitCmd),
    /// 列出工作区 `plugins/*.json` 及校验状态
    List(PluginListCmd),
    /// 校验工作区 `plugins/*.json` 动态工具定义
    Validate(PluginValidateCmd),
}

#[derive(Parser, Debug, Clone)]
pub struct PluginInitCmd {
    /// 工具名（必须 `dyn__` 前缀）
    #[arg(long, value_name = "NAME")]
    pub name: String,
    /// 工具描述
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,
    /// 执行命令（须在 allowed_commands 白名单）
    #[arg(long, value_name = "CMD")]
    pub command: Option<String>,
    /// 固定命令参数（可重复）
    #[arg(long = "arg", value_name = "ARG")]
    pub args: Vec<String>,
    /// 是否在命令尾部追加原始 args_json
    #[arg(long, default_value_t = true)]
    pub pass_args_json: bool,
    /// 输出文件（默认 `plugins/<name-without-prefix>.json`）
    #[arg(long, value_name = "FILE")]
    pub output: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct PluginValidateCmd {
    /// 仅校验指定文件（默认扫描整个 `plugins/*.json`）
    #[arg(long, value_name = "FILE")]
    pub file: Option<String>,
    /// 以 JSON 输出结果（便于 CI 机器读取）
    #[arg(long)]
    pub json: bool,
    /// 以 JSONL 输出结果（每行一个对象，便于管道处理）
    #[arg(long)]
    pub jsonl: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct PluginListCmd {
    /// 仅列出指定文件（默认扫描整个 `plugins/*.json`）
    #[arg(long, value_name = "FILE")]
    pub file: Option<String>,
    /// 以 JSON 输出结果（便于 CI 机器读取）
    #[arg(long)]
    pub json: bool,
    /// 以 JSONL 输出结果（每行一个对象，便于管道处理）
    #[arg(long)]
    pub jsonl: bool,
}

/// 配置检查（不发起对话）
#[derive(Parser, Debug, Clone, Default)]
pub struct ConfigCmd {
    /// 可选；与不带本参数相同，均为一次配置检查后退出（供脚本显式标注）
    #[arg(long)]
    pub dry_run: bool,

    /// 与 `serve --with-web` 同语义：检查 UI 静态目录是否存在（默认跳过，纯 API）。
    #[arg(long = "with-web", alias = "web")]
    pub with_web: bool,
}

/// 将会话 JSON 导出为与 Web 一致的 `chat_export_*.json` / `.md`（**不要**求 `API_KEY`）
#[derive(Parser, Debug, Clone)]
pub struct SaveSessionCmd {
    /// 导出格式（默认两者皆写）
    #[arg(long, value_enum, default_value_t = SaveSessionFormat::Both)]
    pub format: SaveSessionFormat,

    /// JSON 信封 `projection`：`raw`（完整 Message，可 tool-replay）或 `display`（展示投影，不可直接 tool-replay）；默认 `raw`
    #[arg(long, value_enum, default_value_t = SaveSessionProjection::Raw)]
    pub projection: SaveSessionProjection,

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

/// `save-session --projection` 取值（JSON 信封；Markdown 不受影响）
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SaveSessionProjection {
    /// 完整 OpenAI 形 `Message`（默认；可 `tool-replay`）
    #[default]
    Raw,
    /// 展示投影瘦消息（对齐 Web/Tauri；**不可**直接 `tool-replay`）
    Display,
}

/// 解析后的 `save-session` 参数（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub struct SaveSessionCli {
    pub format: SaveSessionFormat,
    pub projection: SaveSessionProjection,
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

/// e2e 真实 LLM 端到端测试子命令。
#[derive(Parser, Debug, Clone)]
pub struct E2eCmd {
    /// e2e 模式：real（默认，调用真实 LLM）、record、replay
    #[arg(long, value_name = "MODE", default_value = "real")]
    pub mode: String,

    /// artifact 输出目录（默认 .crabmate/e2e_artifacts）
    #[arg(long = "output-dir", value_name = "DIR")]
    pub output_dir: Option<String>,

    /// 录制数据目录（默认 tests/fixtures/llm_recordings）
    #[arg(long = "recordings-dir", value_name = "DIR")]
    pub recordings_dir: Option<String>,

    /// 外部场景文件（JSON/YAML），不指定则使用预设场景
    #[arg(long = "scenarios-file", value_name = "FILE")]
    pub scenarios_file: Option<String>,

    /// 启用 LLM-as-Judge 评分（需额外 API 调用）
    #[arg(long)]
    pub judge: bool,
}

/// `e2e` 解析结果（供 `cli_run` 执行）。
#[derive(Debug, Clone)]
pub struct E2eCliArgs {
    pub mode: String,
    pub output_dir: Option<String>,
    pub recordings_dir: Option<String>,
    pub scenarios_file: Option<String>,
    pub judge: bool,
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
    /// `mcp serve`（`--no-tools` / `--port` 见子命令）
    McpServe {
        no_tools: bool,
        port: u16,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// 启动 HTTP API（默认纯 API；可选 `--with-web` 挂载 UI；默认端口 8080）
    Serve(ServeCmd),
    /// 批量 benchmark 测评（JSONL）
    Bench(BenchCmd),
    /// 配置与自检（如 dry-run）
    Config(ConfigCmd),
    /// 一页本地诊断（Rust/npm/前端路径、白名单条数等；人读，脱敏；**不要**求 API_KEY）
    Doctor,
    /// 设置/查询/清除 Web API Bearer（系统钥匙串；**不要**求 API_KEY）
    #[command(name = "web-bearer")]
    WebBearer(WebBearerCmd),
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
    /// 从 SSE 事件录制文件回放 AG-UI 事件到 TurnLayout 投影（**不要**求 API_KEY）
    #[command(name = "sse-replay")]
    SseReplay(SseReplayCmd),
    /// MCP stdio 客户端运维：列出本进程内已缓存会话（**不要**求 API_KEY）
    Mcp(McpCmd),
    /// 动态工具模板与校验（工作区 `plugins/*.json`）
    Plugin(PluginCmd),
    /// 工作流作者层 YAML/Markdown 编译与校验（**不要**求 API_KEY）
    Workflow(WorkflowCmd),
    /// 真实 LLM e2e 端到端测试（需 API_KEY）
    E2e(E2eCmd),
}

#[derive(Parser, Debug)]
#[command(
    name = "crabmate",
    version,
    about = "基于 OpenAI 兼容 chat/completions 的 Rust AI Agent；本仓以 `serve` 为执行权威。官方终端为 Client 仓 `crabmate-tui`",
    after_long_help = "官方对话请：`crabmate serve` + Client `crabmate-tui` / 桌面 / WASM。同进程 `chat|repl|tui` 入口已移除（见 docs/design/client_shell_split.md）。运维子命令与 SSE：**docs/命令行与路由.md**、**docs/SSE协议.md**。"
)]
pub struct RootCli {
    #[command(flatten)]
    pub global: GlobalOpts,

    /// 须显式子命令（如 `serve`、`doctor`）；无默认 `repl`
    #[command(subcommand)]
    pub command: Commands,
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
    pub samples: usize,
    pub resume: bool,
    pub system_prompt_file: Option<String>,
}

/// [`parse_args`](super::parse::parse_args) 的返回值：具名字段替代长元组，便于增删选项与调用方阅读。
#[derive(Debug, Clone)]
pub struct ParsedCliArgs {
    pub config_path: Option<String>,
    pub serve_port: Option<u16>,
    /// `serve --desktop-ready-json` / `--web-ready-json`：监听成功后打印 `web_ready`（已弃用命名；壳不依赖）
    pub serve_desktop_ready_json: bool,
    /// `serve` 时使用；来自 `serve --host`、`CM_HTTP_HOST` 或默认 `127.0.0.1`。
    pub http_bind_host: String,
    pub workspace_cli: Option<String>,
    pub no_tools: bool,
    /// `serve` / `config`：是否挂载或检查业务 UI 静态资源（默认 `false` = 纯 API）。
    pub with_web: bool,
    pub dry_run: bool,
    pub log_file: Option<String>,
    pub bench_args: BenchmarkCliArgs,
    pub extra_cli: ExtraCliCommand,
    /// `Some` 时执行导出后退出（与 `doctor` 一样不要求 API_KEY）
    pub save_session: Option<SaveSessionCli>,
    /// `Some` 时执行 `web-bearer` 后退出（不要求 API_KEY）
    pub web_bearer: Option<WebBearerCli>,
    /// `Some` 时执行工具重放子命令后退出（不要求 API_KEY）
    pub tool_replay: Option<ToolReplayCli>,
    /// `Some` 时执行 SSE replay 回放后退出（不要求 API_KEY）
    pub sse_replay: Option<SseReplayCli>,
    /// `Some` 时执行动态工具模板生成后退出（不要求 API_KEY）
    pub plugin_init: Option<PluginInitCli>,
    /// `Some` 时执行动态工具校验后退出（不要求 API_KEY）
    pub plugin_validate: Option<PluginValidateCli>,
    /// `Some` 时执行动态工具列表后退出（不要求 API_KEY）
    pub plugin_list: Option<PluginListCli>,
    /// `Some` 时执行 `workflow validate` 后退出（不要求 API_KEY）
    pub workflow_validate: Option<WorkflowFileCli>,
    /// `Some` 时执行 `workflow compile` 后退出（不要求 API_KEY）
    pub workflow_compile: Option<WorkflowFileCli>,
    /// `Some` 时执行 `workflow run` 后退出（不要求 API_KEY）
    pub workflow_run: Option<WorkflowFileCli>,
    /// 全局 `--llm-context-tokens`：非零时覆盖已加载配置中的 `llm_context_tokens`
    pub llm_context_tokens_cli: Option<u32>,
    /// `Some` 时执行 e2e 测试后退出（需 API_KEY）
    pub e2e: Option<E2eCliArgs>,
}

/// `plugin init` 解析结果（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub struct PluginInitCli {
    pub name: String,
    pub description: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub pass_args_json: bool,
    pub output: Option<String>,
}

/// `plugin validate` 解析结果（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub struct PluginValidateCli {
    pub file: Option<String>,
    pub json: bool,
    pub jsonl: bool,
}

/// `plugin list` 解析结果（供 `runtime::cli` 执行）
#[derive(Debug, Clone)]
pub struct PluginListCli {
    pub file: Option<String>,
    pub json: bool,
    pub jsonl: bool,
}

/// `sse-replay` 子命令：从 `sse-replay-events.jsonl` 回放 AG-UI 事件并投影为 Web 块布局行。
#[derive(Parser, Debug, Clone)]
pub struct SseReplayCmd {
    /// `sse-replay-events.jsonl` 文件路径
    pub file: String,

    /// 输出格式：rows（默认，投影行列表）或 canonical（canonical Turn 状态）
    #[arg(long, default_value = "rows")]
    pub format: String,

    /// 仅输出指定 job_id 的事件（0 表示全部）
    #[arg(long, default_value_t = 0)]
    pub job_id: u64,
}

/// `sse-replay` 解析结果（供 `runtime::cli` 执行；**不要**求 API_KEY）
#[derive(Debug, Clone)]
pub struct SseReplayCli {
    pub file: String,
    pub format: String,
    pub job_id: u64,
}
