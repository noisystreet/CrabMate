**语言 / Languages:** 中文（本页）· [English](README.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

<p align="center">
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/code-complexity.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/code-complexity.yml/badge.svg?branch=main" alt="code-complexity" /></a>
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml/badge.svg?branch=main" alt="Dependency security" /></a>
  <br />
  <a href="https://github.com/noisystreet/CrabMate/stargazers"><img src="https://img.shields.io/github/stars/noisystreet/CrabMate?style=flat&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/noisystreet/CrabMate/commits/main"><img src="https://img.shields.io/github/last-commit/noisystreet/CrabMate?logo=github" alt="Last commit" /></a>
  <a href="https://github.com/noisystreet/CrabMate/issues"><img src="https://img.shields.io/github/issues/noisystreet/CrabMate" alt="Issues" /></a>
  <a href="https://github.com/noisystreet/CrabMate/pulls"><img src="https://img.shields.io/github/issues-pr/noisystreet/CrabMate" alt="Pull requests" /></a>
  <a href="https://github.com/noisystreet/CrabMate/blob/main/LICENSE"><img src="https://img.shields.io/github/license/noisystreet/CrabMate" alt="License" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust" alt="Rust 1.85+" /></a>
  <a href="https://crates.io/crates/crabmate"><img src="https://img.shields.io/crates/v/crabmate.svg" alt="crates.io" /></a>
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型。

提供 HTTP **`serve`**（默认纯 API）与运维 CLI。**官方 Web UI、Desktop/Android、远程终端（`crabmate-tui`）**在 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**（本机开发默认同级 `../crabmate-client`；Playwright 转发可用 **`CRABMATE_CLIENT_DIR`**）。本仓同进程 **`repl` / `chat` / `tui` 命令入口已移除**（请用 Client **`crabmate-tui`**；见 [ADR](docs/design/client_shell_split.md)）。

**路径 A（双仓）**：本仓维护 **Server**（`serve`、契约、运维 CLI）；官方 Client 在 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**（[ADR](docs/design/client_shell_split.md)）。GitHub 默认展示英文入口见根目录 **[README.md](README.md)**。

## 目录

- [功能概览](#功能概览)
- [常用子命令](#常用子命令)
- [编译运行与打包](#编译运行与打包)
  - [Makefile（推荐）](#makefile推荐)
  - [后端](#后端)
  - [前端 Web](#前端-web)
  - [官方 Client（Desktop / Android）](#官方-clientdesktop-android)
  - [安装与发行包](#安装与发行包)
  - [开发与质检（维护者）](#开发与质检维护者)
- [文档索引](#文档索引)
- [后端模型支持](#后端模型支持)
- [环境变量提示](#环境变量提示)
- [部署与安全](#部署与安全)
- [项目结构](#项目结构)

## 功能概览

- **对话与工具**：OpenAI 兼容 `chat/completions`；内置文件/工作区、**`run_command`**（白名单；默认含 **`bash`/`sh`**：需 glob/`$VAR`/`~` 时经 **`bash -c`** 跑整行脚本；Web 上独立 argv 的 `&&`/`|` 即使 bash 已在白名单也会再审完整脚本；审批对象为完整脚本；argv 含工作区外绝对路径或路径穿越形 `..`/`../` 时默认经 **`allow_external_path_with_approval`** 人工审批后放行，可关；**git** `A..B` 不算穿越）、HTTP、**联网搜索**（默认 **worbrow** 本机浏览器，免 API Key；可选 Brave/Tavily）、工作区**代码检索**（关键字 + 可选语义/向量）等；完整列表见 [docs/工具说明.md](docs/工具说明.md)。**`run_command`** 等子进程工具输出默认按 **`command_max_output_len`**（嵌入默认 **512KiB**）截断，详见 **`config/tools.toml`** 与 [docs/配置说明.md](docs/配置说明.md)。
- **Web UI（Client）**：源码与发版在 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**；本仓 **`serve` 默认纯 API**；同机托管 SPA 须 **`--with-web`**，并用 **`CM_WEB_STATIC_DIR`**（或探测 Client `frontend/dist`）。会话、工作区/项目池、编辑器、PR、终端流聊天、Ask/Plan/Act、设置等见 Client README 与 [docs/命令行与路由.md](docs/命令行与路由.md)。须**显式选择工作区**后工具与 **`@相对路径`** 才生效。助手可用 **`![说明](相对路径.png)`** 内嵌工作区图片（Client 经 **`GET /workspace/file/raw`** 鉴权加载；仅 png/jpg/jpeg/webp/gif）。侧栏「保存到本机」走 **`GET /workspace/file/download`**（任意类型，16 MiB）。本机文件可拖到工作区树，经 **`PUT /workspace/file/raw`** 写入（原始字节，16 MiB）。
- **终端**：官方远程客户端为 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** 的 **`crabmate-tui`**（HTTP/SSE 连本仓 **`serve`**；模型密钥存 Client）。本仓同进程 **`repl` / `chat` / `tui` 已硬删**（D2.2，见 [docs/design/client_shell_split.md](docs/design/client_shell_split.md) §2.5）。**`serve`** 默认纯 API（可选 **`--with-web`**）。流式 **SSE** 见 [docs/SSE协议.md](docs/SSE协议.md)。
- **会话与导出**：嵌入默认在**当前工作区** **`.crabmate/conversations.db`** 持久化 **Web `serve`**；不需要时将 **`conversation_store_sqlite_path`** 置空。Web 或 CLI **`save-session`**（别名 **`export-session`**）导出 JSON/Markdown，形状见 [docs/命令行与路由.md](docs/命令行与路由.md)。
- **进阶（默认不必读）**：分阶段规划、澄清问卷、**`thinking_trace`**、长期记忆、活文档、**MCP**、工作区 **`plugins/*.json`** 等见 [docs/配置说明.md](docs/配置说明.md)、[docs/工具说明.md](docs/工具说明.md)。

## 常用子命令

不写子命令时须显式给出（如 **`serve`**）。请优先 **`serve`** + Client **`crabmate-tui`**。全局常用选项：**`--config`**、**`--workspace`**、**`--no-tools`**、**`--llm-context-tokens`**、**`--log`**（详见 **`crabmate --help`**）。

| 子命令 | 说明 |
| --- | --- |
| **`serve`** | 启动 HTTP API（**默认纯 API，不挂 SPA**）。同机托管 UI：加 **`--with-web`**，并用 **`CM_WEB_STATIC_DIR`**（或探测 Client/`frontend/dist` / 安装路径）。默认端口 **8080**，绑定 **127.0.0.1**。 |
| **`doctor`** | 本机环境与依赖一页诊断（**不要**求 `API_KEY`）。 |
| **`config`** | 加载配置并自检（如 **`--dry-run`**）。 |
| **`models`** / **`probe`** | 探测 **`api_base`** 上 **`GET …/models`**；**`bearer`** 模式下通常需要环境变量 **`API_KEY`**。 |
| **`save-session`** | 从磁盘会话文件导出到 **`<workspace>/.crabmate/exports/`**（别名 **`export-session`**）。 |
| **`bench`** | 批量测评（JSONL）；用法见 [benchmark/README.md](benchmark/README.md)、[docs/基准测试规划.md](docs/基准测试规划.md)。 |
| **`mcp`** | **`mcp list`** / **`mcp list --probe`**；**`mcp serve`** 对外暴露内置工具（stdio，无传输鉴权）。 |
| **`plugin`** | **`init`** / **`list`** / **`validate`**：工作区 **`plugins/*.json`**（**`dyn__`** 前缀）。 |
| **`workflow`** | **`compile`** / **`validate`** / **`run`**：工作区 YAML/Markdown 工作流（**不要**求 `API_KEY`）；见 [docs/工作流编写教程.md](docs/工作流编写教程.md)。 |
| **`tool-replay`** | 从会话导出工具 fixture 或重放（**不要**求 `API_KEY`，须在可信工作区）。 |

完整参数、HTTP 路由与 **`man crabmate`**：[docs/命令行与路由.md](docs/命令行与路由.md)。

## 编译运行与打包

**前置**：**Rust 1.85+**（edition 2024）。业务 UI 在 Client 仓（Trunk / wasm32）。更多环境说明见 [AGENTS.md](AGENTS.md)。

### Makefile（推荐）

```bash
make help              # 列出全部目标
make all / all-dev     # backend-release / backend
make backend           # cargo build -p crabmate
make package           # server-only tar.gz + 可选 .deb → dist/（不附带 UI）
make clean             # 清理 target 与 dist/
```

业务 UI：`cd ../crabmate-client && make frontend`（先将 [crabmate-client](https://github.com/noisystreet/crabmate-client) 克隆为同级目录）。本仓 **`make package`** / **`package-tar`** / **`package-deb`** 默认 **不**打包 frontend（运行时默认纯 API；托管 SPA 用 **`--with-web`** + **`CM_WEB_STATIC_DIR`**）。Desktop / Android：**[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**。

### 后端

```bash
# 开发调试二进制
cargo build
./target/debug/crabmate serve            # 默认纯 API
./target/debug/crabmate serve --with-web # 同机托管 SPA（须 CM_WEB_STATIC_DIR 或可探测 dist）
# 或: API_KEY=… ./target/debug/crabmate serve

# 发布用优化二进制
cargo build --release
./target/release/crabmate serve

# 可选：Ubuntu 24.04 工具链镜像（开发 + `make package`；glibc 2.39；非运行镜像）
# docker build -t crabmate-dev .          # 仅 DNS 异常时再加 --network=host
# docker run --rm -it -v "$PWD":/workspace -w /workspace crabmate-dev
# make package-docker                     # → 宿主 dist/*.tar.gz 与 dist/*.deb
```

**`serve`** 的 Web API 鉴权（**`CM_WEB_API_BEARER_TOKEN`** 等）见 **[部署与安全](#部署与安全)**。调用云端模型所需的 **`API_KEY`** 见 **[环境变量提示](#环境变量提示)**（或 Client 侧栏 / 请求体 **`client_llm`**）。

### 前端 Web

业务 UI 源码在官方 Client 仓 **[`frontend/`](https://github.com/noisystreet/crabmate-client/tree/main/frontend)**（[crabmate-client](https://github.com/noisystreet/crabmate-client)；路径 A Phase 4.2）。本机默认同级 `../crabmate-client`。

```bash
cd ../crabmate-client && make frontend
export CM_WEB_STATIC_DIR="$PWD/frontend/dist"
cd ../crabmate_agent && cargo run -- serve --with-web
```

纯 API（默认）：`serve`。UI 指针见 [`docs/frontend/`](docs/frontend/)。

### 官方 Client（Desktop / Android）

> **权威仓**：**[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**（路径 A；见 [`docs/design/client_shell_split.md`](docs/design/client_shell_split.md)；本机同级 `../crabmate-client`）。  
> 本仓 **已移除** `desktop-tauri/` / `mobile-tauri/` / `crates/crabmate-connect`（Phase 4.1；权威仅在 Client 仓）。

壳**不**拉起 `serve`：先本机或远程启动 **`crabmate serve`**，再在 Client 仓连接页填写服务器与 Web API Bearer。

```bash
cd ../crabmate-client
make desktop-release    # Linux .deb（无 serve sidecar）
# 或 make apk / cargo tauri dev — 见 Client 仓 README
```

兼容矩阵见 [`docs/design/client_compat_matrix.md`](docs/design/client_compat_matrix.md)。

### 安装与发行包

| 方式 | 命令 / 说明 |
| --- | --- |
| **安装到 PATH** | **`cargo install crabmate`**（crates.io **稳定版 `0.4.0`**，默认 feature **`server`**）。源码树 / GitHub 预发布 **`0.5.0-alpha.1`**（`v0.5.0-alpha.1`）：**`cargo install --path .`** 或 Release 的 tar.gz/`.deb`。**不**附带 **man**；可手动安装 **[man/crabmate.1](man/crabmate.1)**。 |
| **一键 tar.gz / .deb** | **`make package`**（或 **`./scripts/package-release.sh --skip-frontend`**）→ **`dist/`**（二进制、`config/`、man、**`systemd/`**、**`etc/crabmate/`**；**默认不附带 UI**）。仅 tar：**`make package-tar`**；仅 deb：**`make package-deb`**（需 **`cargo-deb`**）。脚本仍支持可选 **`--frontend-dist`**，本 Makefile 不走该路径。 |
| **Debian 包** | **`make package-deb`** / **`cargo deb`**（本仓不强制 UI）；产物在 **`dist/`** 或 **`target/debian/`**。安装 **`crabmate.service`**（默认 **127.0.0.1:8080**，纯 API，**不**自动 enable；托管 SPA 用 **`--with-web`** + **`CM_WEB_STATIC_DIR`**）。桌面壳 `.deb` 见 Client 仓。详 [docs/命令行与路由.md](docs/命令行与路由.md)。 |
| **桌面 / APK** | **仅** Client 仓（[`crabmate-client`](https://github.com/noisystreet/crabmate-client)）。 |
| **同步 man 页** | **`cargo run --features gen-man --bin crabmate-gen-man`**（与 clap 帮助对齐）。 |

### 开发与质检（维护者）

- **Cargo features / 裁剪二进制**：默认 **`server`**（含 **`protocol`**、**`web`**、**`mcp`**）；**`fastembed`**、**`project_metrics`**、`docker_sandbox` / `gen-man` 按需开启。同进程 **`repl`/`tui` feature 已移除（D2.2）**，官方终端用 Client **`crabmate-tui`**。语义检索：`cargo build --features fastembed`；tokei：`cargo build --features project_metrics`。完整能力：`cargo build --all-features`。详见根目录 **`Cargo.toml`** **`[features]`**。
- **fmt / clippy / test、pre-commit、SSE、E2E**：见 **[docs/测试指南.md](docs/测试指南.md)**（含 **`./scripts/check-sse-protocol.sh`**）。CI 另跑 **`make package`**（server-only tar.gz + `.deb` 冒烟）。

## 文档索引

| 文档 | 内容 | English |
| --- | --- | --- |
| [CHANGELOG.md](CHANGELOG.md) | 发版说明（Keep a Changelog） | — |
| [docs/开发文档.md](docs/开发文档.md) | 架构概要、主要模块与数据流 | [en](docs/en/DEVELOPMENT.md) |
| [docs/配置说明.md](docs/配置说明.md) | 环境变量、`CM_*`、Web/TOML 详解 | [en](docs/en/CONFIGURATION.md) |
| [docs/工具说明.md](docs/工具说明.md) | 内置工具与调用示例 | [en](docs/en/TOOLS.md) |
| [docs/工作流编写教程.md](docs/工作流编写教程.md) | 工作流 YAML/steps 编写与示例 | — |
| [docs/SSE协议.md](docs/SSE协议.md) | `/chat/stream` 控制面 JSON | [en](docs/en/SSE_PROTOCOL.md) |
| [docs/命令行与路由.md](docs/命令行与路由.md) | 子命令、HTTP 路由、打包 | [en](docs/en/CLI.md) |
| [docs/命令行契约.md](docs/命令行契约.md) | `chat` 退出码与 **`--output json`** | [en](docs/en/CLI_CONTRACT.md) |
| [docs/调试指南.md](docs/调试指南.md) | 日志、`doctor`、`GET /web-ui` 等 | [en](docs/en/DEBUG.md) |
| [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) | 个人自用：本机 `serve` + TLS 反代 + Bearer | — |
| [docs/测试指南.md](docs/测试指南.md) | 测试、pre-commit、审计命令 | [en](docs/en/TESTING.md) |
| [docs/design/client_shell_split.md](docs/design/client_shell_split.md) | 官方 Client 拆分（路径 A） | — |
| [docs/design/frontend_migrate_plan.md](docs/design/frontend_migrate_plan.md) | Phase 4.2：`frontend/` 迁出实施计划 | — |
| [docs/design/client_compat_matrix.md](docs/design/client_compat_matrix.md) | Server ↔ 协议版 ↔ 最低 Client 兼容表 | — |
| [docs/基准测试规划.md](docs/基准测试规划.md) | **`bench`** 规划与开源基准衔接 | — |
| [docs/BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md) | 已记录的 bench 分数（不含密钥） | — |
| [benchmark/README.md](benchmark/README.md) | HumanEval 转换、执行与冒烟 | — |

**更多**：维护待办、路线图、前端架构草案等见 **`docs/`**（一览：[docs/中英文文档对照.md](docs/中英文文档对照.md)）。

**维护约定**：用户可见变更需同步 README 与相关文档，细则见 [docs/开发文档.md](docs/开发文档.md)。

## 后端模型支持

`POST {api_base}/chat/completions`（OpenAI 兼容）。`[agent]` 里配置 **`api_base`**、**`model`**、**`max_tokens`**（嵌入默认 **4096**）、**`llm_http_auth_mode`**；**`bearer`** 时 **`API_KEY`** 走环境变量，**勿**写入仓库配置。

| 场景 | 配置要点 |
| --- | --- |
| **DeepSeek** | `api_base`：`https://api.deepseek.com/v1`；常用 `model` 见 **`config/llm_vendors.toml`**（`deepseek-v4-flash`、`deepseek-v4-pro`、`deepseek-v4-flash-vision-exp` 等）。会话附图仍是 `/uploads/`；出站仅 **vision-exp** 会打成 `data:`。[官网](https://platform.deepseek.com/) · [API](https://api-docs.deepseek.com/api/create-chat-completion) |
| **MiniMax** | `api_base`：`https://api.minimaxi.com/v1`（国际站 `https://api.minimax.io/v1`）；`model` 如 `MiniMax-M3`。[配置说明](docs/配置说明.md) · [厂商 OpenAI 兼容](https://platform.minimax.io/docs/api-reference/text-openai-api) |
| **智谱 GLM** | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5.3`。[配置说明](docs/配置说明.md) · [GLM-5.3](https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3) |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k3`。[配置说明](docs/配置说明.md) · [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) |
| **本地 Ollama 等** | `llm_http_auth_mode = "none"`，`api_base` 如 `http://127.0.0.1:11434/v1`；可不设 `API_KEY`。 |

本机诊断：**`crabmate doctor`**（无需 `API_KEY`）、**`probe`** / **`models`**。各厂商特有选项见 [docs/配置说明.md](docs/配置说明.md)。**厂商能力以供应商文档为准**。

## 环境变量提示

| 变量 | 作用 |
| --- | --- |
| **`API_KEY`** | 云网关 Bearer（**`llm_http_auth_mode=bearer`**）；可选进程回退供 **`serve`** / **`models`** / **`probe`**。官方 Client 对话走请求体 **`client_llm.api_key`**（密钥存 Client 本机）。 |
| **`CM_API_BASE`** / **`CM_MODEL`** | 覆盖配置中的网关与模型。 |
| **`CM_WEB_API_BEARER_TOKEN`** | Web API 保护（与 **`web_api_require_bearer`** 配合）；详见 [docs/配置说明.md](docs/配置说明.md)。 |
| **`CM_WEB_CORS_ALLOWED_ORIGINS`** | 额外 Origin 白名单（逗号分隔）；**未设置**时已默认放行官方壳 Origin（`tauri://localhost`、`http://tauri.localhost`）。显式空串关闭 CORS。静态浏览器 UI：补上其 Origin；见设置页 **API 基址**（`localStorage` **`crabmate-api-base-url`**）。 |
| **`CM_WEB_STATIC_DIR`** | 覆盖 **`serve --with-web`** 时的静态资源根（Client `frontend/dist` / 安装路径；默认不挂 SPA）。 |
| **`CM_DESKTOP_SUGGESTED_URL`** | 可选：桌面连接页预填的 `serve` URL（默认 `http://127.0.0.1:8080/`）。 |
| **`CM_DESKTOP_SERVE_URL`** | 跳过连接页时必填：已运行的 `serve` URL（配合 **`CM_DESKTOP_SKIP_CONNECT`** / **`CM_E2E_FIXTURES`**）。 |

其它 **`CM_*`**（skills、分阶段规划等）见 [docs/配置说明.md](docs/配置说明.md)。

## 部署与安全

- **监听**：默认 **`127.0.0.1`**；监听 **`0.0.0.0`** 须 **`web_api_bearer_token`** 或显式不安全开关（见 [docs/配置说明.md](docs/配置说明.md)）。
- **LLM API Key**：Client 本机存放并经 **`client_llm.api_key`** 发送；进程环境变量 **`API_KEY`** 仍可作为 **`serve`** / 运维回退。
- **Web API**：嵌入默认 **`web_api_require_bearer = false`**，允许无共享密钥启动 **`serve`**；若设为 **`true`**，则启动前须配置非空 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML / **`crabmate web-bearer set`**）。密钥非空时请求须带 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**。浏览器须在 **设置 →「Web API 共享密钥（Bearer）」** 保存与服务端相同的值（**`localStorage`** **`crabmate-api-bearer-token`**），**不要**与模型 **`API_KEY`** 混淆。**跨 Origin 静态 UI**：设置页填 **API 基址**；官方壳 Origin 已默认放行，其它浏览器 Origin 再配 **`CM_WEB_CORS_ALLOWED_ORIGINS`**。冒烟见 **`docs/design/client_turn_smoke_runbook.md`** §9。**本机临时跳过**：`unset CM_WEB_API_BEARER_TOKEN` 后听 `127.0.0.1`；或清密钥后设 **`CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`** 再听 `0.0.0.0`。对外建议 **`web_api_require_bearer = true`**。详见 [docs/配置说明.md](docs/配置说明.md)。
- **其它**：Web 侧栏「设置」须 **「保存全部」** 才写入；工作区须在允许根内。调试与 **`GET /web-ui`** 见 [docs/调试指南.md](docs/调试指南.md)。
- **个人 VPS（反代 TLS）**：见 [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md)。

## 项目结构

架构分层、主要模块与数据流概要见 [docs/开发文档.md](docs/开发文档.md)；**`GET /status`** 返回完整运行状态；Web 壳层请用 **`GET /status?view=shell`**。其它观测字段见 [docs/调试指南.md](docs/调试指南.md)。

- **单 crate**：crates.io **稳定版**仍为 **`0.4.0`**（[crates.io/crates/crabmate](https://crates.io/crates/crabmate)，默认 **`server`**）。**`cargo install crabmate`** 仍装该版。本树为 **`0.5.0-alpha.1`**（git tag **`v0.5.0-alpha.1`**，GitHub **prerelease** 产物）。官方 Client 钉 **`version = "0.4.0", default-features = false, features = ["protocol"]`**（仅 `crabmate::cm_sse_protocol`、`cm_types` 等，不要用 `types`/`sse` 别名）。
- **semver 面**：`protocol` 为六个 `cm_*` 契约模块；`server` 承诺组合面模块**名**（`agent` / `config` / `llm` / `sse` / `types`）与根上显式 `pub use`（`run`、`run_agent_turn`、`build_tools*` 等）。`#[doc(hidden)]` 模块与 `agent::agent_turn` 等内部路径**不是**稳定 SDK。详见 [docs/design/crates_io_single_package.md](docs/design/crates_io_single_package.md) §2.4。
