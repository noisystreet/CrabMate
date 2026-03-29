# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate logo" width="220" />
</p>

基于 **DeepSeek API** 的 Rust AI Agent，支持 **Function Calling**、工作区内命令与文件工具、Web UI 与 CLI。

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；切换模型见配置。
- **内置工具**：文件、命令、HTTP、联网搜索、多语言开发辅助等；**能力与 JSON 参数示例**见 [`docs/TOOLS.md`](docs/TOOLS.md)。
- **可选 MCP（stdio）**：配置 `mcp_enabled` + `mcp_command` 后合并远端工具为 `mcp__{slug}__{tool}`；同一进程内复用一条 stdio 连接（`serve` / `repl` / `chat` 多轮共用）。运维可 **`crabmate mcp list`** 查看本进程已缓存会话与合并后的工具名（**不要**求 `API_KEY`）；**`mcp list --probe`** 会按配置尝试连接一次（排障用，会启动 `mcp_command` 子进程）。`mcp_command` 等效允许启动子进程，须可信配置。
- **Web UI**：聊天、工作区浏览/编辑（`GET /workspace/file` 可选 **`encoding`** 查询参数，与 `read_file` 一致，用于 GBK/GB18030 等 legacy 文本）、任务清单（进程内 `/tasks`，重启清空）、状态栏；Agent 改文件后列表自动刷新。
- **项目画像**：侧栏只读摘要（`Cargo.toml` / `package.json`、目录与 tokei 等）；可与工作区备忘合并注入新会话首轮（`project_profile_inject_*`）。模型也可用内置工具 **`repo_overview_sweep`** 拉取同源画像（见 `include_project_profile` / `project_profile_max_chars`，[`docs/TOOLS.md`](docs/TOOLS.md)）。
- **流式与审批**：Web SSE；`run_command` 与未匹配前缀的 **`http_fetch` / `http_request`** 等可走 `POST /chat/approval`。CLI（repl/chat）下非白名单 **`run_command`** 与未匹配前缀的 **`http_fetch` / `http_request`** 走同一套终端审批（TTY 为 **dialoguer** 菜单，管道/无头读一行 **`y`/`a`/`n`**；或 **`--yes`** / **`--approve-commands`**，后者仅命令名）。**Web 与 CLI 对照表**见 [`docs/CLI.md`](docs/CLI.md)「CLI 与 Web 能力对照」。
- **会话与导出**：Web 可选 `conversation_id` + **`conversation_store_sqlite_path`** 持久化（TTL/条数上限见配置），并可在 UI **导出 JSON/MD**。CLI 可用 **`crabmate save-session`**（兼容别名 **`export-session`**；默认读工作区 **`.crabmate/tui_session.json`**，写入 **`.crabmate/exports/`**，与前端同形），REPL **`/save-session`**（与上述子命令同逻辑）或 **`/export`**（导出当前内存中的对话）。**`crabmate tool-replay`** 可从会话 JSON 提取工具调用序列为 fixture 并重放（复现/回归，不调用大模型；见 [`docs/CLI.md`](docs/CLI.md)）。REPL 可选从 **`tui_session.json`** 恢复（`tui_load_session_on_start`）。默认按会话累积**工具写入路径 + unified diff 摘要**并在每轮请求模型前注入（**`session_workspace_changelist_*`**，保存会话前剥离）。备忘 **`agent_memory_file`**、**长期记忆**见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。会话/审批差异仍以 **`docs/CLI.md`** 对照节为准。

## 文档索引

| 文档 | 内容 |
|------|------|
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | 架构、模块索引、协议与扩展点 |
| [`docs/TOOLS.md`](docs/TOOLS.md) | 内置工具说明与调用示例 |
| [`docs/SSE_PROTOCOL.md`](docs/SSE_PROTOCOL.md) | `/chat/stream` 控制面 JSON |
| [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) | 环境变量、`AGENT_*`、规划/上下文等配置详解 |
| [`docs/CLI.md`](docs/CLI.md) | 子命令、选项、HTTP 路由、打包 deb |
| [`docs/CLI_CONTRACT.md`](docs/CLI_CONTRACT.md) | `chat` 退出码、`--output json` 行协议、与 SSE 错误码交叉引用 |
| [`docs/TODOLIST.md`](docs/TODOLIST.md) | 未完成待办：全局 P0–P5 + 按模块分章（完成后从清单删除） |

维护约定：用户可见变更需同步 README / DEVELOPMENT / TOOLS 等，见 `DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

## 部署与安全（摘要）

- **默认仅本机**：`serve` 绑定 `127.0.0.1`。`0.0.0.0` 时须配置 **`web_api_bearer_token`**，或显式允许 `allow_insecure_no_auth_for_non_loopback`（不安全）。
- **Bearer**：设置后主要 API 需 `Authorization: Bearer`；前端可从 `localStorage["crabmate-api-bearer-token"]` 读取。
- **工作区路径**：须在允许的根目录内；每次请求重验。未配白名单时仅允许 `run_command_working_dir` 下路径。无鉴权时不要暴露在不可信网络。
- **联网搜索 Key**：`web_search_api_key` 与 DeepSeek `API_KEY` 分离，注意文件权限。
- **可选 Docker 工具沙盒**：将 SyncDefault 与部分工具（含 `run_command` 等，在宿主白名单/审批后）放到一次性容器内执行；需本机 Docker、**自管镜像**（镜像提供 CLI 依赖，宿主 `crabmate` 二进制只读挂入容器）。完整步骤、镜像要求、网络与 `user` 见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)「SyncDefault 工具 Docker 沙盒」。

更细的边界与敏感面见 **`docs/DEVELOPMENT.md`** 与 **`.cursor/rules/security-sensitive-surface.mdc`**（维护者）。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 `AGENTS.md`）
- **环境变量**：`API_KEY` — DeepSeek API Key（`doctor` 除外）

```bash
export API_KEY="your-api-key"
cargo run              # 默认进入 repl
cargo run -- serve     # Web，默认 8080
```

**REPL**：默认 `cargo run` 进入交互模式；启动时打印**分节摘要**（模型与鉴权、`api_base`、流式开关、工作区/工具、内建命令列表、要点配置如 `max_tokens` / 分阶段规划等）；运行中可输入 **`/config`** 再次打印关键配置摘要（不含密钥），**`/config reload`** 从磁盘与环境变量热更配置（见 **`docs/CONFIGURATION.md`**），**`/doctor`**、**`/probe`**、**`/models`** 分别等价于 **`crabmate doctor`**、**`crabmate probe`**、**`crabmate models`**（详见 [`docs/CLI.md`](docs/CLI.md)）。**`/mcp`** 与 **`crabmate mcp list`** 对齐（**`/mcp probe`** 尝试连接）；**`/version`** 打印版本与 OS/ARCH。开启分阶段规划时，若不想在终端打印**无工具规划轮**的模型原文，可设 **`staged_plan_cli_show_planner_stream = false`** 或 **`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM=0`**（仍保留步骤队列摘要与后续执行步输出；见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)）。默认在首轮 `agent_reply_plan` 后还会多一次无工具**步骤优化**轮（合并无依赖只读探查步等），可用 **`staged_plan_optimizer_round = false`** 或 **`AGENT_STAGED_PLAN_OPTIMIZER_ROUND=0`** 关闭以省 API。可选将 **SyncDefault** 及 **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** 在宿主审批/白名单后放入 **Docker** 执行（`sync_default_tool_sandbox_mode = docker` + 镜像名等；默认在 Unix 上以**当前有效 uid:gid** 作为容器 `user` 以减轻工作区文件属主问题，见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)「SyncDefault 工具 Docker 沙盒」）。交互 TTY 下 **空行按 `$`（或 `$` + Enter）** 进入 **`bash#:`** 本机 shell 一行（`sh -c` / `cmd /C`），**不等同**于模型的 `run_command` 白名单，仅适合可信环境；管道输入仍可用 **`$ <命令>`**；行编辑与历史见 [`docs/CLI.md`](docs/CLI.md)「行首 `$`」。可选设置 **`AGENT_CLI_WAIT_SPINNER=1`**：在等待模型首包流式输出期间于 **stderr** 显示 spinner 与耗时（须 TTY、未设 **`NO_COLOR`**），详见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。内建命令的成功/错误反馈行首为 **✓**/**✗**；**NO_COLOR** 或非 TTY 下为 **`[ok]`/`[err]`**（纯 ASCII，避免缺字终端乱码）。**Tab** 可补全以 **`/`** 开头的内建命令（**`bash#:`** 下关闭），见 [`docs/CLI.md`](docs/CLI.md)。

前端：`cd frontend && npm install && npm run build` 后再 `serve`（静态资源来自 `frontend/dist`）。

**配置**：`default_config.toml` + 可选 `config.toml`；默认通过 **`system_prompt_file = "prompts/default_system_prompt.md"`** 从仓库文件加载（**改该文件无需重编**；相对路径会按当前目录、配置文件所在目录、`run_command_working_dir` 依次解析，见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)）。**环境变量与高级项**同见该文档。**子命令、Benchmark、deb 包、`man` 手册页**见 [`docs/CLI.md`](docs/CLI.md)（troff 源为 **`man/crabmate.1`**，与 `clap` 不同步时执行 **`cargo run --bin crabmate-gen-man`**；**`cargo install`** 默认不安装 man，**`cargo deb`** 会装入 **`/usr/share/man/man1`**）。

**本地模型（如 Ollama）**：`api_base` 指向其 OpenAI 兼容根（如 `http://127.0.0.1:11434/v1`），并设 **`llm_http_auth_mode = "none"`**（或 `AGENT_LLM_HTTP_AUTH_MODE=none`）即可不设 **`API_KEY`**；详见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。

## 项目结构

源码模块与调用关系见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（含 Mermaid 与 `src/` 索引）。

## 上下文管道（观测）

Web **`GET /status`** 会返回 **`message_pipeline_trim_count_hits`**、**`message_pipeline_trim_char_budget_hits`**、**`message_pipeline_tool_compress_hits`**、**`message_pipeline_orphan_tool_drops`**（自进程启动以来的累计命中次数，非单会话）。排障时可设 **`RUST_LOG=crabmate::message_pipeline=trace`** 查看逐步 `session_sync_step`（详见 `docs/DEVELOPMENT.md`）。若启用 **`context_char_budget`** 且 **`context_min_messages_after_system` ≥ `max_message_history`**，加载配置时会 **`warn`**，提示按字符删旧消息往往难以生效。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
