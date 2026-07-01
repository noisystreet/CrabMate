**语言 / Languages:** 中文（本页）· [English](README-en.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

<p align="center">
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/code-complexity.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/code-complexity.yml/badge.svg" alt="code-complexity" /></a>
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml/badge.svg" alt="Dependency security" /></a>
  <a href="https://github.com/noisystreet/CrabMate/blob/main/LICENSE"><img src="https://img.shields.io/github/license/noisystreet/CrabMate" alt="License" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust" alt="Rust 1.85+" /></a>
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型。

内置 **Function Calling** 与工作区内的命令、文件等工具，并提供 **Web UI** 与 **CLI**。

## 目录

- [功能概览](#功能概览)
- [常用子命令](#常用子命令)
  - [TUI（全屏终端）](#tui全屏终端)
- [编译运行与打包](#编译运行与打包)
  - [Makefile（推荐）](#makefile推荐)
  - [后端](#后端)
  - [前端 Web](#前端-web)
  - [桌面 Tauri](#桌面-tauri)
  - [安装与发行包](#安装与发行包)
  - [开发与质检（维护者）](#开发与质检维护者)
- [文档索引](#文档索引)
- [后端模型支持](#后端模型支持)
- [环境变量提示](#环境变量提示)
- [部署与安全](#部署与安全)
- [项目结构](#项目结构)

## 功能概览

- **对话与工具**：OpenAI 兼容 `chat/completions`；内置文件/工作区、**`run_command`**（白名单；默认含 **`bash`/`sh`**，复合命令用 **`bash -c`/`sh -c`**）、HTTP/搜索、工作区**代码检索**（关键字 + 可选语义/向量）等；完整列表见 [docs/工具说明.md](docs/工具说明.md)。**`run_command`** 等子进程工具输出默认按 **`command_max_output_len`**（嵌入默认 **512KiB**，`CM_COMMAND_MAX_OUTPUT_LEN` 可覆盖）做字节截断，详见 **`config/tools.toml`** 与 [docs/配置说明.md](docs/配置说明.md)。
- **Web UI**：侧栏会话与工作区；侧栏底部或移动顶栏可切换主区 **「对话 / 编辑器」**（编辑器模式为左侧工作区树 + 右侧文本编辑，单击打开、**双击将 `@相对路径` 插入对话并切回对话布局**；支持 **Ctrl/Cmd+S 保存**、**全部保存**、**新建文件**；Agent 改盘后可通过 SSE 触发已打开文件的磁盘同步提示；偏好存浏览器）；须**显式选择工作区**后工具与 **`@相对路径`** 才生效；浏览器内会话列表按**当前工作区根路径**分桶保存在 `localStorage`，切换工作区会加载该路径下曾保存的会话（未设置工作区前仍使用与旧版相同的默认键）。助手 **Markdown**；支持 **`@` 引用**、图片附件（须视觉模型）、会话导出等。与服务器会话同步后，底栏可显示当前消息的 **prompt** tiktoken 粗估用量及相对 **`llm_context_tokens`** 上限的占比（详见 `title` 提示）。全屏「设置」中 **「会话」** 页可切换本进程是否将服务端会话写入 SQLite（与配置文件中 **`conversation_store_sqlite_path`** 是否可启用一致；**重启 `serve`** 后仍以配置文件为准），并可设置界面与聊天正文字体（仅存本机浏览器、即时生效）。详细路由与行为见 [docs/命令行与路由.md](docs/命令行与路由.md)。
- **终端**：**`repl`**（交互）、**`chat`**（单次）、**`serve`**（HTTP + 静态 UI）、**`tui`**（实验性**全屏**，须真实 TTY，见下文）。流式 **SSE**、工具审批与取消约定见 [docs/SSE协议.md](docs/SSE协议.md)。
- **会话与导出**：嵌入默认在**当前工作区** **`.crabmate/conversations.db`** 持久化 **Web `serve`**（及配置了同路径的 **`tui`**）对话，**`serve` 重启**后仍可按 **`conversation_id`** 续聊；不需要时在配置里将 **`conversation_store_sqlite_path`** 置空。Web 或 CLI **`save-session`**（别名 **`export-session`**）导出 JSON/Markdown，形状见 [docs/命令行与路由.md](docs/命令行与路由.md)。
- **进阶（默认不必读）**：分阶段规划时间线、澄清问卷、调试台 **`thinking_trace`**、长期记忆、活文档注入、**MCP**、工作区 **`plugins/*.json`** 等见 [docs/配置说明.md](docs/配置说明.md)、[docs/工具说明.md](docs/工具说明.md)。

## 常用子命令

不写子命令时默认进入 **`repl`**。全局常用选项：**`--config`**、**`--workspace`**、**`--no-tools`**、**`--agent-role`**、**`--llm-context-tokens`**、**`--log`**（详见 **`crabmate --help`**）。

| 子命令 | 说明 |
| --- | --- |
| **`serve`** | 启动 HTTP API + 挂载 **`frontend/dist`** Web UI（默认端口 **8080**，绑定 **127.0.0.1**）。 |
| **`repl`** | 交互式终端对话；**`/`** 斜杠命令与 **`/api-key set`** 等见 [docs/命令行与路由.md](docs/命令行与路由.md)。 |
| **`chat`** | 单次提问后退出（**`--query`** / **`--stdin`** / 文件等），适合脚本；**`--output json`** 见 [docs/命令行契约.md](docs/命令行契约.md)。 |
| **`tui`** | 实验性**全屏**终端 UI；须**交互式 TTY**（管道或非 TTY 请用 **`repl`** / **`chat`**）。行为摘要见 **[TUI（全屏终端）](#tui全屏终端)**。 |
| **`doctor`** | 本机环境与依赖一页诊断（**不要**求 `API_KEY`）。 |
| **`config`** | 加载配置并自检（如 **`--dry-run`**）。 |
| **`models`** / **`probe`** | 探测 **`api_base`** 上 **`GET …/models`**；**`bearer`** 模式下通常需要环境变量 **`API_KEY`**。 |
| **`save-session`** | 从磁盘会话文件导出到 **`<workspace>/.crabmate/exports/`**（别名 **`export-session`**）。 |
| **`bench`** | 批量测评（JSONL）；用法见 [benchmark/README.md](benchmark/README.md)、[docs/基准测试规划.md](docs/基准测试规划.md)。 |
| **`mcp`** | **`mcp list`** / **`mcp list --probe`**；**`mcp serve`** 对外暴露内置工具（stdio，无传输鉴权）。 |
| **`plugin`** | **`init`** / **`list`** / **`validate`**：工作区 **`plugins/*.json`** 动态工具（**`dyn__`** 前缀）。 |
| **`workflow`** | **`compile`** / **`validate`** / **`run`**：工作区 YAML/Markdown 工作流（**不要**求 `API_KEY`）；见 [docs/工作流编写教程.md](docs/工作流编写教程.md)。 |
| **`tool-replay`** | 从会话导出工具 fixture 或重放（**不要**求 `API_KEY`，须在可信工作区）。 |

完整参数、HTTP 路由与 **`man crabmate`**：[docs/命令行与路由.md](docs/命令行与路由.md)。

### TUI（全屏终端）

**`crabmate tui`** 为实验性**全屏**界面，与 **`repl`** 共用 Agent/工具编排；适合在终端里查看**工作区 / 任务 / 变更预览**而不开浏览器。

- **环境**：须真实 **TTY**；否则请用 **`repl`** / **`chat`**。
- **交互**：撰写区 **Enter** 发送；右栏 **「工作区」** 聚焦时 **Enter** 打开路径浏览（与 Web **`/workspace`**、REPL **`/workspace`** 同源）。**`q`** / **Ctrl+C** 退出。**`/api-key`** 等 **`/`** 命令与 **`repl`** 同源。
- **流式**：不在 **stdout** 刷助手流式正文；细节与 **`--no-stream`** 见 **`crabmate tui --help`**。
- **其它**：可选 SQLite 多会话（**`/conv`**、**`/branch`**）、澄清问卷、环境变量 **`CM_TUI_CONVERSATION_ID`**、退出会话文件等见 **[docs/命令行与路由.md](docs/命令行与路由.md)**。

## 编译运行与打包

**前置**：**Rust 1.85+**（edition 2024）；带 Web 时需安装 [**Trunk**](https://trunkrs.dev/) 并添加目标 **`wasm32-unknown-unknown`**（**`rustup target add wasm32-unknown-unknown`**）。更多环境说明见 [AGENTS.md](AGENTS.md)。

### Makefile（推荐）

仓库根目录提供 **`Makefile`**，可统一构建后端、前端、桌面与工作区，并支持清理：

```bash
make help              # 列出全部目标
make all-dev           # 后端 + 前端（debug，本地 serve 常用）
make all               # 后端 + 前端 + 桌面（均为 release）
make backend           # cargo build -p crabmate
make frontend-release  # cd frontend && trunk build --release
make desktop-dev       # Tauri 开发（需 cargo install tauri-cli --version "^2"）
make clean             # 清理 target、frontend/dist、桌面产物与 dist/
```

桌面构建会自动设置 **`CM_DESKTOP_BACKEND_BIN`**，并将 **`frontend/dist`** 同步到 **`desktop-tauri/dist`**。发布用 **`make all`** 与下文分步命令等价；一键 tar.gz 仍可用 **`./scripts/package-release.sh`**。

### 后端

```bash
# 开发调试二进制
cargo build
./target/debug/crabmate serve    # 或 repl / chat …

# 发布用优化二进制
cargo build --release
./target/release/crabmate serve
```

**`serve`** 的 Web API 鉴权（**`CM_WEB_API_BEARER_TOKEN`** 等）见 **[部署与安全](#部署与安全)**。调用云端模型所需的 **`API_KEY`** 见 **[环境变量提示](#环境变量提示)**（或通过 Web「设置」、REPL **`/api-key set`**）。

### 前端 Web

静态资源由 **`crabmate serve`** 从 **`frontend/dist`** 提供，无需单独起前端进程。

```bash
cd frontend
trunk build              # 开发构建；发布用 trunk build --release
```

然后回到仓库根目录执行 **`crabmate serve`**（或 **`cargo run -- serve`**）。开发细节见 **`frontend/README.md`**。

### 桌面 Tauri

目录：**`desktop-tauri/`**。**WebView** 加载由壳进程拉起的 **`serve`**（**`--port 0 --desktop-ready-json`**，解析 stdout 中 **`web_ready`** 再打开 URL；见 [**desktop-tauri/README.md**](desktop-tauri/README.md)）。若 **`crabmate`** 不在 **`PATH`**，设置 **`CM_DESKTOP_BACKEND_BIN`** 指向已编译后端。

```bash
cargo build
cd frontend && trunk build && cd ..
cargo install tauri-cli --version "^2"   # 仅需一次
cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/绝对路径/到/target/debug/crabmate cargo tauri dev
```

发布：**`cargo tauri build`**。代理与故障排查见 [**desktop-tauri/DEVELOPMENT.md**](desktop-tauri/DEVELOPMENT.md)。

### 安装与发行包

| 方式 | 命令 / 说明 |
| --- | --- |
| **安装到 PATH** | **`cargo install --path .`**（**不**附带 **man**；可手动安装 **[man/crabmate.1](man/crabmate.1)**）。 |
| **一键 tar.gz** | **`./scripts/package-release.sh`** → **`dist/crabmate_<version>_<os>_<arch>.tar.gz`**（含二进制、`config/`、`frontend/dist`、man）；若已装 **`cargo-deb`** 可同时收录 **`.deb`**。 |
| **Debian 包** | 前端 **`trunk build --release`** 后 **`cargo deb`**，产物默认在 **`target/debian/`**。详 [docs/命令行与路由.md](docs/命令行与路由.md)。 |
| **桌面（Tauri）** | 打桌面安装包（当前配置默认产出 **Linux `.deb`**，见 **`desktop-tauri/src-tauri/tauri.conf.json`** 的 **`bundle.targets`**）；步骤见下。 |
| **同步 man 页** | **`cargo run --bin crabmate-gen-man`**（与 clap 帮助对齐）。 |

**Tauri 桌面打包（示例，仓库根目录执行）：**

```bash
cargo build --release
cd frontend && trunk build --release && cd ..
rm -rf desktop-tauri/dist && cp -r frontend/dist desktop-tauri/dist

cd desktop-tauri/src-tauri
# beforeBuildCommand 会运行 ../scripts/prepare-sidecar.sh；也可手动：bash ../scripts/prepare-sidecar.sh
cargo tauri build
```

说明：**`prepare-sidecar.sh`** 会把 **`target/release/crabmate`**（或环境变量 **`CM_DESKTOP_BACKEND_BIN`**）复制到 **`desktop-tauri/binaries/`**，供应用作为 **sidecar** 启动后端。桌面 `.deb` 还会安装 `/etc/crabmate/config.toml`、**`/etc/crabmate/agent_roles.toml`**（多角色）与配套 **`/etc/crabmate/prompts/*.md`**、**`/etc/crabmate/config/prompts/*.md`**；应用启动后端时若检测到 `/etc/crabmate/config.toml` 会自动追加 `--config /etc/crabmate/config.toml`。构建完成后安装包一般在 **`desktop-tauri/src-tauri/target/release/bundle/deb/`**（具体文件名随 **`productName`** / 版本变化）。跨平台 **`bundle.targets`**、代理与 **`GDK_BACKEND`** 等见 [**desktop-tauri/DEVELOPMENT.md**](desktop-tauri/DEVELOPMENT.md)。

### 开发与质检（维护者）

- **Cargo features / 裁剪二进制**：默认 **`mcp` + `fastembed` + `web` + `repl` + `tui`**（`docker_sandbox` / `gen-man` 按需开启）。瘦构建示例：`cargo build --no-default-features --features web,repl,tui`（不链接 ONNX / MCP）。详见根目录 **`Cargo.toml`** **`[features]`**。
- **fmt / clippy / test、pre-commit、SSE 回归脚本、E2E**：命令汇总见 **[docs/测试指南.md](docs/测试指南.md)**（含 **`./scripts/check-sse-protocol.sh`**）。

## 文档索引

| 文档 | 内容 | English |
| --- | --- | --- |
| [docs/开发文档.md](docs/开发文档.md) | 架构概要、主要模块与数据流 | [en](docs/en/DEVELOPMENT.md) |
| [docs/配置说明.md](docs/配置说明.md) | 环境变量、`CM_*`、Web/TOML 详解 | [en](docs/en/CONFIGURATION.md) |
| [docs/工具说明.md](docs/工具说明.md) | 内置工具与调用示例 | [en](docs/en/TOOLS.md) |
| [docs/工作流编写教程.md](docs/工作流编写教程.md) | 工作流 YAML/steps 编写与示例 | — |
| [docs/SSE协议.md](docs/SSE协议.md) | `/chat/stream` 控制面 JSON | [en](docs/en/SSE_PROTOCOL.md) |
| [docs/命令行与路由.md](docs/命令行与路由.md) | 子命令、HTTP 路由、deb 打包 | [en](docs/en/CLI.md) |
| [docs/命令行契约.md](docs/命令行契约.md) | `chat` 退出码与 **`--output json`** | [en](docs/en/CLI_CONTRACT.md) |
| [docs/调试指南.md](docs/调试指南.md) | 日志、`doctor`、`GET /web-ui` 等 | [en](docs/en/DEBUG.md) |
| [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) | 个人自用：本机 `serve` + TLS 反代 + Bearer | — |
| [docs/测试指南.md](docs/测试指南.md) | 测试、pre-commit、审计命令 | [en](docs/en/TESTING.md) |
| [docs/基准测试规划.md](docs/基准测试规划.md) | **`bench`** 规划与开源基准衔接 | — |
| [benchmark/README.md](benchmark/README.md) | HumanEval 转换、执行与冒烟 | — |

**更多**：维护待办、路线图、前端架构草案、中英文文档索引等见 **`docs/`**（一览：[docs/中英文文档对照.md](docs/中英文文档对照.md)）。

**维护约定**：用户可见变更需同步 README 与相关文档，细则见 [docs/开发文档.md](docs/开发文档.md)。

## 后端模型支持

`POST {api_base}/chat/completions`（OpenAI 兼容）。`[agent]` 里配置 **`api_base`**、**`model`**、**`max_tokens`**（嵌入默认 **4096**）、**`llm_http_auth_mode`**；**`bearer`** 时 **`API_KEY`** 走环境变量，**勿**写入仓库配置。

| 场景 | 配置要点 |
| --- | --- |
| **DeepSeek** | `api_base`：`https://api.deepseek.com/v1`；`model` 如 `deepseek-chat` / `deepseek-reasoner`。[官网](https://platform.deepseek.com/) · [API](https://api-docs.deepseek.com/api/create-chat-completion) |
| **MiniMax** | `api_base`：`https://api.minimaxi.com/v1`；`model` 如 `MiniMax-M2.7`。[配置说明](docs/配置说明.md) · [厂商 OpenAI 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api) |
| **智谱 GLM** | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5`。[配置说明](docs/配置说明.md) · [GLM-5](https://docs.bigmodel.cn/cn/guide/models/text/glm-5) |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k2.5`。[配置说明](docs/配置说明.md) · [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) |
| **本地 Ollama 等** | `llm_http_auth_mode = "none"`，`api_base` 如 `http://127.0.0.1:11434/v1`；可不设 `API_KEY`。 |

本机诊断：**`crabmate doctor`**（无需 `API_KEY`）、**`probe`** / **`models`**。各厂商特有选项（thinking、temperature 钳制等）见 [docs/配置说明.md](docs/配置说明.md)。**厂商能力以供应商文档为准**。

## 环境变量提示

| 变量 | 作用 |
| --- | --- |
| **`API_KEY`** | 云网关 Bearer token（**`llm_http_auth_mode=bearer`**）；`serve` / `repl` / `chat` 可先启动再在界面或 **`/api-key`** 设置。 |
| **`CM_API_BASE`** / **`CM_MODEL`** | 覆盖配置中的网关与模型。 |
| **`CM_WEB_API_BEARER_TOKEN`** | Web API 保护（与 **`web_api_require_bearer`** 配合）；详见 [docs/配置说明.md](docs/配置说明.md)。 |

其它 **`CM_*`**（含 **`CM_TUI_CONVERSATION_ID`**、skills、分阶段规划等）见 [docs/配置说明.md](docs/配置说明.md)。

## 部署与安全

- **监听**：默认 **`127.0.0.1`**；监听 **`0.0.0.0`** 须 **`web_api_bearer_token`** 或显式不安全开关（见 [docs/配置说明.md](docs/配置说明.md)）。
- **Web API**：嵌入默认 **`web_api_require_bearer = false`**，允许无共享密钥启动 **`serve`**；若设为 **`true`**，则启动前须配置非空 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML **`web_api_bearer_token`**）。密钥非空时会挂载 Bearer 层，请求须带 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**。前端可存 **`localStorage["crabmate-api-bearer-token"]`**。对外或不可信网络建议 **`web_api_require_bearer = true`** 并配置密钥。
- **其它**：Web 侧栏「设置」须 **「保存全部」** 才写入浏览器；工作区须在允许根内（路径校验见 [docs/配置说明.md](docs/配置说明.md)）。调试变量与 **`GET /web-ui`** 见 [docs/调试指南.md](docs/调试指南.md)。
- **个人 VPS（反代 TLS）**：见 [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md)（**`127.0.0.1` + `CM_WEB_API_BEARER_TOKEN` + Caddy/Nginx**）。

## 项目结构

架构分层、主要模块与数据流概要见 [docs/开发文档.md](docs/开发文档.md)；**`GET /status`** 等观测见 [docs/调试指南.md](docs/调试指南.md)。

- **Workspace 成员**：`crates/crabmate-sse-protocol`（SSE 控制面契约）；**`crates/crabmate-im-bridge`**（可选 **IM 桥**：飞书 Webhook → **`POST /chat`** → 回复）。说明见 [docs/design/feishu_bridge_mvp.md](docs/design/feishu_bridge_mvp.md)。
