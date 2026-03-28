# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate logo" width="220" />
</p>

基于 **DeepSeek API** 的 Rust AI Agent，支持 **Function Calling**、工作区内命令与文件工具、Web UI 与 CLI。

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；切换模型见配置。
- **内置工具**：文件、命令、HTTP、联网搜索、多语言开发辅助等；**能力与 JSON 参数示例**见 [`docs/TOOLS.md`](docs/TOOLS.md)。
- **可选 MCP（stdio）**：配置 `mcp_enabled` + `mcp_command` 后合并远端工具为 `mcp__{slug}__{tool}`；`mcp_command` 等效允许启动子进程，须可信配置。
- **Web UI**：聊天、工作区浏览/编辑、任务清单（进程内 `/tasks`，重启清空）、状态栏；Agent 改文件后列表自动刷新。
- **项目画像**：侧栏只读摘要（`Cargo.toml` / `package.json`、目录与 tokei 等）；可与工作区备忘合并注入新会话首轮（`project_profile_inject_*`）。
- **流式与审批**：Web SSE；`run_command` / `http_fetch` 等可走 `POST /chat/approval`。CLI 下 `run_command` 非白名单用终端确认（或 `--yes` / `--approve-commands`）；**`http_fetch` 等在 CLI 无交互审批**，须匹配配置前缀。**Web 与 CLI 对照表**见 [`docs/CLI.md`](docs/CLI.md)「CLI 与 Web 能力对照」。
- **会话与导出**：Web 可选 `conversation_id` + **`conversation_store_sqlite_path`** 持久化（TTL/条数上限见配置），并可在 UI **导出 JSON/MD**。CLI REPL 可选 **`.crabmate/tui_session.json`**（`tui_load_session_on_start`），**无**与 Web 同形的一键导出子命令。备忘 **`agent_memory_file`**、**长期记忆**见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。会话/审批/导出差异仍以 **`docs/CLI.md`** 对照节为准。

## 文档索引

| 文档 | 内容 |
|------|------|
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | 架构、模块索引、协议与扩展点 |
| [`docs/TOOLS.md`](docs/TOOLS.md) | 内置工具说明与调用示例 |
| [`docs/SSE_PROTOCOL.md`](docs/SSE_PROTOCOL.md) | `/chat/stream` 控制面 JSON |
| [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) | 环境变量、`AGENT_*`、规划/上下文等配置详解 |
| [`docs/CLI.md`](docs/CLI.md) | 子命令、选项、HTTP 路由、打包 deb |
| [`docs/TODOLIST.md`](docs/TODOLIST.md) | 未完成待办（完成后从清单删除） |

维护约定：用户可见变更需同步 README / DEVELOPMENT / TOOLS 等，见 `DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

## 部署与安全（摘要）

- **默认仅本机**：`serve` 绑定 `127.0.0.1`。`0.0.0.0` 时须配置 **`web_api_bearer_token`**，或显式允许 `allow_insecure_no_auth_for_non_loopback`（不安全）。
- **Bearer**：设置后主要 API 需 `Authorization: Bearer`；前端可从 `localStorage["crabmate-api-bearer-token"]` 读取。
- **工作区路径**：须在允许的根目录内；每次请求重验。未配白名单时仅允许 `run_command_working_dir` 下路径。无鉴权时不要暴露在不可信网络。
- **联网搜索 Key**：`web_search_api_key` 与 DeepSeek `API_KEY` 分离，注意文件权限。

更细的边界与敏感面见 **`docs/DEVELOPMENT.md`** 与 **`.cursor/rules/security-sensitive-surface.mdc`**（维护者）。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 `AGENTS.md`）
- **环境变量**：`API_KEY` — DeepSeek API Key（`doctor` 除外）

```bash
export API_KEY="your-api-key"
cargo run              # 默认进入 repl
cargo run -- serve     # Web，默认 8080
```

**REPL**：默认 `cargo run` 进入交互模式；行首 **`$`** 为**本机 shell 一行**（`sh -c` / `cmd /C`），**不等同**于模型的 `run_command` 白名单，仅适合可信环境——详见 [`docs/CLI.md`](docs/CLI.md)「行首 `$`」。

前端：`cd frontend && npm install && npm run build` 后再 `serve`（静态资源来自 `frontend/dist`）。

**配置**：`default_config.toml` + 可选 `config.toml`；**环境变量与高级项**见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。**子命令、Benchmark、deb 包**见 [`docs/CLI.md`](docs/CLI.md)。

**本地模型（如 Ollama）**：`api_base` 指向其 OpenAI 兼容根（如 `http://127.0.0.1:11434/v1`），并设 **`llm_http_auth_mode = "none"`**（或 `AGENT_LLM_HTTP_AUTH_MODE=none`）即可不设 **`API_KEY`**；详见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。

## 项目结构

源码模块与调用关系见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（含 Mermaid 与 `src/` 索引）。

## 上下文管道（观测）

Web **`GET /status`** 会返回 **`message_pipeline_trim_count_hits`**、**`message_pipeline_trim_char_budget_hits`**、**`message_pipeline_tool_compress_hits`**、**`message_pipeline_orphan_tool_drops`**（自进程启动以来的累计命中次数，非单会话）。排障时可设 **`RUST_LOG=crabmate::message_pipeline=trace`** 查看逐步 `session_sync_step`（详见 `docs/DEVELOPMENT.md`）。若启用 **`context_char_budget`** 且 **`context_min_messages_after_system` ≥ `max_message_history`**，加载配置时会 **`warn`**，提示按字符删旧消息往往难以生效。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
