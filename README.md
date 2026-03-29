# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate logo" width="300" />
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 **`chat/completions`** 对接 DeepSeek、MiniMax、本地 Ollama 等后端大模型；内置 **Function Calling** 以及工作区内的命令、文件等工具，并提供 **Web UI** 与 **CLI**。

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；切换模型见配置。
- **内置工具**：文件、命令、HTTP、联网搜索、多语言开发辅助等；**能力与 JSON 参数示例**见 [`docs/TOOLS.md`](docs/TOOLS.md)。**`cargo_test` / `npm run test`** 以及部分 **`run_command cargo test`**：进程内可按「源码指纹 + 参数」复用上一次的截断输出，并标注 **缓存命中**（`test_result_cache_*`，见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)）。
- **CLI（终端）**：默认 **`cargo run`** / **`crabmate repl`** 进入**交互式 REPL**（流式对话、内建命令如 **`/config`** / **`/doctor`** / **`/probe`** / **`/models`**、可选 **`bash#:`** 一行 shell）；**`crabmate chat`** 适合脚本与管道的**单次提问**；**`crabmate serve`** 与 Web 共用同一套 Agent 与工具逻辑。另有 **`doctor`**（环境自检）、**`config`**（配置干跑）、**`bench`**、**`save-session`** / **`export-session`**、**`tool-replay`**、**`mcp list`** 等子命令，全局选项含 **`--config`**、**`--workspace`**、**`--no-tools`**、**`--no-stream`** 等。完整列表、退出码与 **`man crabmate`** 见 **[`docs/CLI.md`](docs/CLI.md)**。
- **Web UI**：聊天、工作区浏览/编辑（`GET /workspace/file` 可选 **`encoding`** 查询参数，语义与 `read_file` 一致，便于 GBK/GB18030 等 legacy 文本）、任务清单（进程内 `/tasks`，重启后清空）、状态栏；Agent 写入文件后，工作区列表会自动刷新。
- **项目画像**：侧栏只读摘要（`Cargo.toml` / `package.json`、目录与 tokei 等）；可与工作区备忘合并注入新会话首轮（`project_profile_inject_*`）。另可选注入 **`cargo metadata` 解析的 workspace 内 crate 依赖图**（Mermaid + 结构化 JSON）与根目录 / `frontend/package.json` 的依赖名节选（`project_dependency_brief_inject_*`，与 Web / `repl` / `chat` 首轮同源）。模型也可用内置工具 **`repo_overview_sweep`** 拉取同源画像（见 `include_project_profile` / `project_profile_max_chars`，[`docs/TOOLS.md`](docs/TOOLS.md)）。
- **流式与审批**：Web SSE；`run_command` 与未匹配前缀的 **`http_fetch` / `http_request`** 等可走 `POST /chat/approval`。客户端断开或协作取消时，在 SSE 仍可投递的情况下可能收到控制面 **`error` + `code: STREAM_CANCELLED`**（与协议表见 [`docs/SSE_PROTOCOL.md`](docs/SSE_PROTOCOL.md)）。CLI（repl/chat）下非白名单 **`run_command`** 与未匹配前缀的 **`http_fetch` / `http_request`** 走同一套终端审批（TTY 为 **dialoguer** 菜单，管道/无头读一行 **`y`/`a`/`n`**；或 **`--yes`** / **`--approve-commands`**，后者仅命令名）。**Web 与 CLI 对照表**见 [`docs/CLI.md`](docs/CLI.md)「CLI 与 Web 能力对照」。
- **会话与导出**：Web 可选 `conversation_id` + **`conversation_store_sqlite_path`** 持久化（TTL/条数上限见配置），并可在 UI **导出 JSON/MD**。CLI 可用 **`crabmate save-session`**（兼容别名 **`export-session`**；默认读工作区 **`.crabmate/tui_session.json`**，写入 **`.crabmate/exports/`**，与前端同形），REPL **`/save-session`**（与上述子命令同逻辑）或 **`/export`**（导出当前内存中的对话）。**`crabmate tool-replay`** 可从会话 JSON 提取工具调用序列为 fixture 并重放（复现/回归，不调用大模型；见 [`docs/CLI.md`](docs/CLI.md)）。REPL 可选从 **`tui_session.json`** 恢复（`tui_load_session_on_start`）；默认**不**在后台构建 `initial_workspace_messages`（无首轮项目画像/依赖摘要注入），需设 **`repl_initial_workspace_messages_enabled = true`**（或 **`AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED`**）方启用。默认按会话累积**工具写入路径 + unified diff 摘要**并在每轮请求模型前注入（**`session_workspace_changelist_*`**，保存会话前剥离）。备忘 **`agent_memory_file`**、**长期记忆**见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)。会话/审批差异仍以 **`docs/CLI.md`** 对照节为准。
- **可选 MCP（stdio）**：配置 `mcp_enabled` + `mcp_command` 后合并远端工具为 `mcp__{slug}__{tool}`；同一进程内复用一条 stdio 连接（`serve` / `repl` / `chat` 多轮共用）。运维可 **`crabmate mcp list`** 查看本进程已缓存的会话与合并后的工具名（**不**需要 `API_KEY`）；**`mcp list --probe`** 会按配置尝试连接一次（用于排障，会启动 `mcp_command` 子进程）。`mcp_command` 会启动子进程，请在可信环境下配置。

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

## 后端模型支持

CrabMate 使用 **`POST {api_base}/chat/completions`**（OpenAI 兼容形态，含可选流式 SSE、**tools** / **tool_calls**；具体能力以各供应商为准）。通过 **`[agent] api_base`**、**`model`** 与 **`llm_http_auth_mode`** 切换网关；密钥仅通过环境变量 **`API_KEY`** 传入（`bearer` 时），**不要**把真实密钥写入仓库配置文件。

| 场景 | 配置要点 |
|------|----------|
| **DeepSeek** | `api_base` : `https://api.deepseek.com/v1`；**`model`**（已测）： **`deepseek-chat`**（通用对话）与 **`deepseek-reasoner`**（推理模型，响应可含 **`reasoning_content`** 思维链）；具体以 [DeepSeek 开放平台](https://platform.deepseek.com/) 当前可用模型名为准。参考 [Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)。 |
| **MiniMax** | `api_base` : `https://api.minimaxi.com/v1`；**`model`**（已测）：**`MiniMax-M2.7`**、**`MiniMax-M2.7-highspeed`**、**`MiniMax-M2.5`**；其它型号以控制台为准。线上常见 **`invalid message role: system`**，建议在 **`[agent]`** 中设 **`llm_fold_system_into_user = true`**（将系统提示并入 `user`；嵌入默认 **`false`**，与默认 DeepSeek 模型一致）。若需将思维链与正文分开，可设 **`llm_reasoning_split = true`**（`AGENT_LLM_REASONING_SPLIT`）。说明与示例见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)「MiniMax」与 [MiniMax OpenAI 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api)。 |
| **本地 Ollama 等** | `api_base` 如 `http://127.0.0.1:11434/v1`；**`llm_http_auth_mode = "none"`**（或 **`AGENT_LLM_HTTP_AUTH_MODE=none`**），可不设 **`API_KEY`**。工具调用是否稳定取决于模型与 Ollama 版本。 |

在本机可运行 **`crabmate doctor`**（**不**需要 **`API_KEY`**）、**`crabmate probe`**、**`crabmate models`**，检查鉴权模式、连通性及 **`GET …/models`** 列表（`bearer` 模式下会携带 Bearer）。完整的 **`AGENT_*`**、热重载与边界说明见 **[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)**。

**具体模型的能力、限制与调用方式**（如可用模型 ID、采样参数取值、多模态、供应商专有字段等），**请以各模型厂商官方网站公布的 API 文档为准**；本节与项目内配置文档仅说明在 CrabMate 中如何填写 `api_base` / `model` 等对接项。

## 部署与安全（摘要）

- **默认仅本机**：`serve` 绑定 `127.0.0.1`。监听 `0.0.0.0` 时须配置 **`web_api_bearer_token`**，或显式开启 `allow_insecure_no_auth_for_non_loopback`（**不安全**，仅建议在可信网络下临时使用）。
- **Bearer**：设置后主要 API 需 `Authorization: Bearer`；前端可从 `localStorage["crabmate-api-bearer-token"]` 读取。
- **工作区路径**：须在允许的根目录内；每次请求重验。未配白名单时仅允许 `run_command_working_dir` 下路径。无鉴权时不要暴露在不可信网络。
- **联网搜索 Key**：`web_search_api_key` 与主对话所用 **`API_KEY`** 分离，注意文件权限。
- **可选 Docker 工具沙盒**：将 SyncDefault 与部分工具（含 `run_command` 等，在宿主白名单/审批后）放到一次性容器内执行；需本机 Docker、**自管镜像**（镜像提供 CLI 依赖，宿主 `crabmate` 二进制只读挂入容器）。完整步骤、镜像要求、网络与 `user` 见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)「SyncDefault 工具 Docker 沙盒」。

更细的边界与敏感面见 **`docs/DEVELOPMENT.md`** 与 **`.cursor/rules/security-sensitive-surface.mdc`**（维护者）。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 `AGENTS.md`）
- **环境变量**：`API_KEY` — 云端 OpenAI 兼容网关的 Bearer 密钥（`llm_http_auth_mode=bearer` 时；`doctor` / `save-session` 等除外，见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)）

```bash
export API_KEY="your-api-key"
cargo run              # 默认进入 repl
cargo run -- serve     # Web，默认 8080
```

**REPL**：**`cargo run`**（省略子命令）默认进入**交互式终端对话**；启动时打印模型、工作区与内建命令等**分节摘要**，运行中以 **`/`** 开头的命令（如 **`/config`**、**`/config reload`**、**`/doctor`**、**`/probe`**、**`/models`**、**`/mcp`**、**`/version`**）及可选 **`bash#:`** 本地一行 shell 等，详见 **[`docs/CLI.md`](docs/CLI.md)**「**REPL 内建命令**」（含 **Tab** 补全、**`$`** 切换、分阶段规划终端输出、**SyncDefault Docker**、等待 **spinner**、**`✓`/`[ok]`** 反馈样式等）。对应配置键见 **[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)**。

**前端**：先执行 `cd frontend && npm install && npm run build`，再启动 `serve`（静态资源来自 `frontend/dist`）。

**配置**：`config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml`（编译嵌入）+ 可选 `config.toml`；默认通过 **`system_prompt_file = "config/prompts/default_system_prompt.md"`** 从仓库文件加载（**改该文件无需重编**；相对路径会按当前目录、配置文件所在目录、`run_command_working_dir` 依次解析，见 [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md)）。**环境变量与高级项**同见该文档。**子命令与 Benchmark** 见 [`docs/CLI.md`](docs/CLI.md)；**release 构建、`cargo deb`、`man` 页**见下文 **[「源码编译与打包」](#源码编译与打包)**。

**切换模型 / 网关**（DeepSeek、MiniMax、Ollama 等）：见上文 **[「后端模型支持」](#后端模型支持)**。

## 源码编译与打包

- **工具链**：后端需 **Rust 1.85+**（edition 2024）；构建 Web 前端需 **Node.js / npm**。Linux 上系统库与链接说明（如 `libssl-dev`、`libssh2`，以及长期记忆、可选 ONNX / `g++` 等）见 **`AGENTS.md`**。
- **调试构建**：仓库根目录执行 **`cargo build`**，二进制位于 **`target/debug/crabmate`**。
- **发布构建**：**`cargo build --release`**，二进制位于 **`target/release/crabmate`**。若以 **`serve`** 提供 Web UI，须先构建前端：**`cd frontend && npm install && npm run build`**（输出 **`frontend/dist`**），再启动后端。
- **检查与测试**（维护者/CI）：**`cargo fmt --all`**、**`cargo clippy --all-targets --all-features -- -D warnings`**、**`cargo test`**；前端类型检查 **`cd frontend && npx tsc -b --noEmit`**。仓库 **pre-commit** 配置见 **`.pre-commit-config.yaml`**。
- **安装到本机前缀**：**`cargo install --path .`**（在克隆目录下；默认安装 release 二进制到 `~/.cargo/bin`）。**`cargo install`** **不会**自动安装 **`man`** 页；可手动安装 **`man/crabmate.1`** 或优先使用下方 **`.deb`**。
- **手册页**：`clap` 与 troff 不同步时，在仓库根执行 **`cargo run --bin crabmate-gen-man`** 再提交更新后的 **`man/crabmate.1`**。
- **Debian `.deb` 包**：需安装 [**`cargo-deb`**](https://github.com/kornelski/cargo-deb)（**`cargo install cargo-deb`**），完成前端 **`npm run build`** 后 **`cargo build --release`**，再 **`cargo deb`**；生成的包在 **`target/debian/`**，安装示例 **`sudo dpkg -i target/debian/crabmate_*.deb`**。包内附带默认配置片段、**`README.md`** 与 **`/usr/share/man/man1/crabmate.1`**。细则与路由表见 **[`docs/CLI.md`](docs/CLI.md)**「打包 Debian `.deb`」。

## 项目结构

源码模块与调用关系见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（含 Mermaid 图与 `src/` 索引）。**消息同步管道**在 **`GET /status`** 中的计数字段及 **`RUST_LOG` 排障**，见该文档 **架构设计** 小节「**上下文管道（观测）**」。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
- **MiniMax**：[开放平台 / 文档中心](https://platform.minimaxi.com)
