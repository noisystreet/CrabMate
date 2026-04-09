**语言 / Languages:** 中文（本页）· [English](README-en.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型。

内置 **Function Calling** 与工作区内的命令、文件等工具，并提供 **Web UI** 与 **CLI**。

## 目录

- [功能概览](#功能概览)
- [文档索引](#文档索引)
- [后端模型支持](#后端模型支持)
- [环境与快速开始](#环境与快速开始)
- [源码编译与打包](#源码编译与打包)
- [部署与安全](#部署与安全)
- [项目结构](#项目结构)
- [参考](#参考)

## 功能概览

- **对话与多模型**：OpenAI 兼容 `chat/completions`；网关与模型见配置及下文「后端模型支持」。

- **内置工具**（**Function Calling**）：文件与工作区、**`run_command`**（白名单）、HTTP/搜索、格式化、依赖图与覆盖率、容器封装等；覆盖 **Rust / Python / JS·TS / Go / JVM / C·C++** 等栈及 **GitHub `gh_*`**。全表与 JSON 示例：[docs/TOOLS.md](docs/TOOLS.md)。

- **CLI**：**`crabmate repl`** / **`chat`** / **`serve`**（与 Web 共用 Agent/工具）。详 **[CLI](#cli)**、[docs/CLI.md](docs/CLI.md)。

- **Web UI**：类 DeepSeek 布局；助手 **Markdown**；侧栏会话（导出、筛选、搜索）、工作区树与变更预览、任务与上下文状态、消息多选/重试/分支；**多角色**等见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

- **项目画像**：侧栏摘要与可选首轮注入；模型可用 **`repo_overview_sweep`**（[docs/TOOLS.md](docs/TOOLS.md)）。

- **OpenAPI**：**`GET /openapi.json`**；流式控制面以 [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md) 为准（含 **`client_sse_protocol`** 协商）。

- **流式与审批**：Web **SSE** + **`POST /chat/approval`**；CLI 终端审批；取消码等与 [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md)、[docs/CLI.md](docs/CLI.md)「CLI 与 Web 能力对照」。

- **会话与导出**：Web 可选 SQLite 持久化、导出 JSON/MD；CLI **`save-session`** / **`tool-replay`** 等。工作区变更注入、长期记忆等见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

- **可选**：进程内工具统计（**`agent_tool_stats_enabled`**）；**MCP stdio**（**`mcp_enabled`** + **`mcp_command`**，`crabmate mcp list`）。见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

## 文档索引

| 文档 | 内容 | English |
| --- | --- | --- |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | 架构、模块索引、协议与扩展点 | [en](docs/en/DEVELOPMENT.md) |
| [docs/TOOLS.md](docs/TOOLS.md) | 内置工具说明与调用示例 | [en](docs/en/TOOLS.md) |
| [docs/SSE_PROTOCOL.md](docs/SSE_PROTOCOL.md) | `/chat/stream` 控制面 JSON | [en](docs/en/SSE_PROTOCOL.md) |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | 环境变量、`AGENT_*`、规划/上下文等配置详解 | [en](docs/en/CONFIGURATION.md) |
| [docs/CLI.md](docs/CLI.md) | 子命令、选项、HTTP 路由、打包 deb | [en](docs/en/CLI.md) |
| [docs/CLI_CONTRACT.md](docs/CLI_CONTRACT.md) | `chat` 退出码、`--output json` 行协议、与 SSE 错误码交叉引用 | [en](docs/en/CLI_CONTRACT.md) |
| [docs/TODOLIST.md](docs/TODOLIST.md) | 未完成待办：全局 P0–P5 + 按模块分章（完成后从清单删除） | [en](docs/en/TODOLIST.md) |
| [docs/CODEBASE_INDEX_PLAN.md](docs/CODEBASE_INDEX_PLAN.md) | 统一代码索引与增量缓存规划 | [en](docs/en/CODEBASE_INDEX_PLAN.md) |

**维护约定**：用户可见变更需同步 README 与相关文档，细则见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)「TODOLIST 与功能文档约定」。

## 后端模型支持

`POST {api_base}/chat/completions`（OpenAI 兼容）。`[agent]` 里配置 **`api_base`**、**`model`**、**`llm_http_auth_mode`**；**`bearer`** 时 **`API_KEY`** 走环境变量，**勿**写入仓库配置。

| 场景 | 配置要点 |
| --- | --- |
| **DeepSeek** | `api_base`：`https://api.deepseek.com/v1`；`model` 如 `deepseek-chat` / `deepseek-reasoner`。以 [官网](https://platform.deepseek.com/) 与 [API 文档](https://api-docs.deepseek.com/api/create-chat-completion) 为准。 |
| **MiniMax** | `api_base`：`https://api.minimaxi.com/v1`；`model` 如 `MiniMax-M2.7` 等。system 角色合并、`llm_reasoning_split` 默认等见 [CONFIGURATION「MiniMax」](docs/CONFIGURATION.md) 与 [厂商 OpenAI 兼容说明](https://platform.minimaxi.com/docs/api-reference/text-openai-api)。 |
| **智谱 GLM** | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5`。可选 `llm_bigmodel_thinking`。详 [CONFIGURATION](docs/CONFIGURATION.md)、[GLM-5](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)。 |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k2.5`。temperature 钳制、`llm_kimi_thinking_disabled` 等见 [CONFIGURATION](docs/CONFIGURATION.md)、[Kimi API](https://platform.moonshot.cn/docs/api/chat)。 |
| **本地 Ollama 等** | `llm_http_auth_mode = "none"`，`api_base` 如 `http://127.0.0.1:11434/v1`；可不设 `API_KEY`。 |

本机诊断：**`crabmate doctor`**（无需 `API_KEY`）、**`probe`** / **`models`**。完整 **`AGENT_*`** 与热重载见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。**厂商能力以供应商 API 文档为准**。

## 环境与快速开始

- **Rust**：1.85+（edition 2024，见 [AGENTS.md](AGENTS.md)）

- **Docker 开发环境**（可选）：仓库 [Dockerfile](Dockerfile)（Ubuntu 24.04，Rust + trunk 等）。`docker build -t crabmate-dev .` 后 `docker run --rm -it -v "$(pwd)":/workspace -w /workspace crabmate-dev`；UID/GID 可用 `--build-arg DEV_UID` / `DEV_GID`。**未**预装 pre-commit / Node。

- **环境变量**：**`API_KEY`**（bearer 时；`serve` / `repl` / `chat` 可先启动，对话前在 Web「设置」或 REPL **`/api-key set`**）；**`models` / `probe`** 在 bearer 下通常仍需环境变量里的 Key。**`AGENT_API_BASE`** / **`AGENT_MODEL`** 覆盖配置。完整表见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

```bash
# 可选：export AGENT_API_BASE=… AGENT_MODEL=… API_KEY=…（或 Web「设置」/ REPL /api-key set）
cargo build
./target/debug/crabmate repl    # 安装到 PATH 后可直接 crabmate repl
cd frontend-leptos && trunk build && cd ..
./target/debug/crabmate serve   # 默认 8080；发布前端用 trunk build --release
```

### CLI

- **`crabmate repl`**：交互式对话；**`/`** 内建命令与可选 **`bash#:`** 见 [docs/CLI.md](docs/CLI.md)。bearer 无密钥时提示 **`/api-key`**。
- **`crabmate chat`**：单次非交互；**`serve`**：HTTP + Web UI（与 Web 共用逻辑）。
- **常用**：**`doctor`**、**`config`**、**`probe`** / **`models`**、**`bench`**、**`save-session`** / **`export-session`**、**`tool-replay`**、**`mcp list`**。全局选项 **`--config`**、**`--workspace`**、**`--agent-role`**、**`--no-tools`**、**`--no-stream`** 等。
- 配置键：[docs/CONFIGURATION.md](docs/CONFIGURATION.md)；子命令全表、Benchmark、**`man crabmate`**：[docs/CLI.md](docs/CLI.md)。

**前端**：`cd frontend-leptos && trunk build`（开发；**`--release`** 用于发布），再 **`crabmate serve`**。界面语言在「设置」；详 `frontend-leptos/README.md`、`docs/DEVELOPMENT.md`。

**配置**：默认 `config/*.toml`（编译嵌入）+ 可选根目录 **`config.toml`**；**`system_prompt_file`** 指向 `config/prompts/default_system_prompt.md`（改后不必重编）。高级项见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。**release / deb / man** 见 **[源码编译与打包](#源码编译与打包)**。

**切换模型 / 网关**（DeepSeek、MiniMax、Ollama 等）：见上文 **[「后端模型支持」](#后端模型支持)**。

## 源码编译与打包

- **工具链**：**Rust 1.85+**、**Trunk** + **`wasm32-unknown-unknown`**；Linux / 长期记忆等见 [AGENTS.md](AGENTS.md)。
- **构建**：`cargo build` → `target/debug/crabmate`；**`--release`** → `target/release/crabmate`。带 Web 时先 **`cd frontend-leptos && trunk build`**（发布用 **`--release`**）。
- **检查**：`cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`；或 [.pre-commit-config.yaml](.pre-commit-config.yaml)。
- **E2E**（可选）：`frontend-leptos` 构建后 **`cd e2e && npm ci && npx playwright install chromium && npm test`**。见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。
- **安装**：`cargo install --path .`（**不**自动装 man；`.deb` 包或手动 [man/crabmate.1](man/crabmate.1)）。同步 clap 与 troff：`cargo run --bin crabmate-gen-man`。
- **`.deb`**：[cargo-deb](https://github.com/kornelski/cargo-deb)，前端 release 构建 + **`cargo deb`**，产物 **`target/debian/`**。详 [docs/CLI.md](docs/CLI.md)「打包 Debian `.deb`」。

## 部署与安全

- **监听**：默认 **`127.0.0.1`**；`0.0.0.0` 须 **`web_api_bearer_token`** 或显式不安全开关（见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）。
- **Bearer**：API 鉴权；前端可读 **`localStorage["crabmate-api-bearer-token"]`**。
- **Web「设置」**：本机 **`client_llm`**（`api_base` / `model` / 密钥）仅影响当次请求，详 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)「Web 对话队列」。
- **工作区**：须在允许根内；Unix 上尽力用 **`openat2`** 等收窄路径风险，**非**绝对沙箱。见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)、[`src/path_workspace.rs`](src/path_workspace.rs)。
- **其它**：**`web_search_api_key`** 与主 **`API_KEY`** 分离；可选 **SyncDefault Docker 沙盒**见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。维护者另见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)、[.cursor/rules/security-sensitive-surface.mdc](.cursor/rules/security-sensitive-surface.mdc)。

## 项目结构

模块与调用链、**`GET /status` 观测**、**`src/`** 索引见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
- **MiniMax**：[开放平台 / 文档中心](https://platform.minimaxi.com)
- **智谱 GLM**：[开放平台](https://open.bigmodel.cn/) · [GLM-5 使用指南](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)
- **Moonshot Kimi**：[Kimi API / Chat](https://platform.moonshot.cn/docs/api/chat)
