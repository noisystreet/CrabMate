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
- **流式与审批**：Web SSE；`run_command` 等可走 `POST /chat/approval`。CLI 非白名单命令有终端确认（或 `--yes` / `--approve-commands`，仅可信环境）。
- **会话**：可选 `conversation_id` + **`conversation_store_sqlite_path`** 持久化（TTL/条数上限见配置）。备忘文件 **`agent_memory_file`**、**长期记忆**（SQLite + 可选 `fastembed`）见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。
- **导出**：Web 可导出与 TUI 同形 JSON 及 Markdown 聊天记录。

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

前端：`cd frontend && npm install && npm run build` 后再 `serve`（静态资源来自 `frontend/dist`）。

**配置**：`default_config.toml` + 可选 `config.toml`；**环境变量与高级项**见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。**子命令、Benchmark、deb 包**见 [`docs/CLI.md`](docs/CLI.md)。

## 项目结构

源码模块与调用关系见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（含 Mermaid 与 `src/` 索引）。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
