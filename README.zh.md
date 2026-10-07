**语言 / Languages:** 中文（本页）· [English](README.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

<p align="center">
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml"><img src="https://github.com/noisystreet/CrabMate/actions/workflows/dependency-security.yml/badge.svg?branch=main" alt="Dependency security" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust" alt="Rust 1.85+" /></a>
  <a href="https://crates.io/crates/crabmate"><img src="https://img.shields.io/crates/v/crabmate.svg" alt="crates.io" /></a>
  <a href="https://github.com/noisystreet/CrabMate/blob/main/LICENSE"><img src="https://img.shields.io/github/license/noisystreet/CrabMate" alt="License" /></a>
</p>

**CrabMate** 是基于 Rust 编写的 AI Agent，通过 **OpenAI 兼容** 的 `chat/completions` 对接 DeepSeek、MiniMax、智谱 GLM、Moonshot Kimi、本地 Ollama 等后端大模型。

本仓是 **Server**：HTTP **`serve`**（永远纯 API，不托管 SPA）、`protocol` 契约与运维 CLI。官方 **Web UI、Desktop/Android、远程终端（`crabmate-tui`）**在 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**（本机开发默认同级 `../crabmate-client`）；本仓同进程 `repl` / `chat` / `tui` 已移除（[ADR](docs/design/client_shell_split.md)）。

## 目录

- [CrabMate](#crabmate)
  - [目录](#目录)
  - [功能概览](#功能概览)
  - [快速开始](#快速开始)
  - [常用子命令](#常用子命令)
  - [后端模型支持](#后端模型支持)
  - [部署与安全](#部署与安全)
  - [安装与发行包](#安装与发行包)
  - [文档](#文档)

## 功能概览

- **对话与工具**：OpenAI 兼容 `chat/completions`；内置工作区文件工具、**`run_command`**（仅白名单）、HTTP、**联网搜索**（默认本机 **worbrow** 浏览器，免 API Key；可选 Brave/Tavily）、工作区**代码检索**（关键字 + 可选语义）。完整列表与语义见 [docs/工具说明.md](docs/工具说明.md)。
- **会话**：**`serve`** 默认在 **`<workspace>/.crabmate/conversations.db`** 持久化对话；保留策略、删除与导出可配置——见 [docs/配置说明.md](docs/配置说明.md)、[docs/命令行与路由.md](docs/命令行与路由.md)。
- **Client UI**：由 **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** 构建并托管；浏览器 UI 经 CORS 接入。含会话、工作区/项目池、编辑器与 PR 视图、终端流聊天、Ask/Plan/Act、设置。
- **进阶（按需开启）**：分阶段规划、澄清问卷、**`thinking_trace`**、长期记忆、活文档、**MCP**、工作区 **`plugins/*.json`**——见 [docs/配置说明.md](docs/配置说明.md)。

## 快速开始

需要 **Rust 1.85+**（edition 2024）。

```bash
# 后端（纯 API）
cargo build
./target/debug/crabmate serve            # 默认 127.0.0.1:8080；或：API_KEY=… ./target/debug/crabmate serve

# Client UI（独立仓）
cd ../crabmate-client && make frontend   # 再指向 http://127.0.0.1:8080/ 并填 Bearer
```

推荐用 Makefile：**`make help`**、**`make backend`**（`cargo build -p crabmate`）、**`make package`**（server-only tar.gz + 可选 `.deb` → `dist/`）。

## 常用子命令

不写子命令时须显式给出（如 **`serve`**）。请优先 **`serve`** + Client **`crabmate-tui`**。全局常用选项：**`--config`**、**`--workspace`**、**`--no-tools`**、**`--llm-context-tokens`**、**`--log`**（详见 **`crabmate --help`**）。

| 子命令 | 说明 |
| --- | --- |
| **`serve`** | 启动 HTTP API（**永远纯 API**，不托管 SPA）。UI 由 Client 仓托管并经 CORS 接入。默认端口 **8080**，绑定 **127.0.0.1**。 |
| **`doctor`** | 本机环境与依赖一页诊断（**不要**求 `API_KEY`）。 |
| **`config`** | 加载配置并自检。 |
| **`models`** / **`probe`** | 探测 **`api_base`** 上 **`GET …/models`**；**`bearer`** 模式下通常需要环境变量 **`API_KEY`**。 |
| **`save-session`** | 从磁盘会话文件导出到 **`<workspace>/.crabmate/exports/`**（别名 **`export-session`**）。 |
| **`bench`** | 批量测评（JSONL）；用法见 [benchmark/README.md](benchmark/README.md)、[docs/基准测试规划.md](docs/基准测试规划.md)。 |
| **`mcp`** | **`mcp list`** / **`mcp list --probe`**；**`mcp serve`** 对外暴露内置工具（stdio，无传输鉴权）。 |
| **`plugin`** | **`init`** / **`list`** / **`validate`**：工作区 **`plugins/*.json`**（**`dyn__`** 前缀）。 |
| **`workflow`** | **`compile`** / **`validate`** / **`run`**：工作区 YAML/Markdown 工作流（**不要**求 `API_KEY`）；见 [docs/工作流编写教程.md](docs/工作流编写教程.md)。 |
| **`tool-replay`** | 从会话导出工具 fixture 或重放（**不要**求 `API_KEY`，须在可信工作区）。 |

完整参数、HTTP 路由与 **`man crabmate`**：[docs/命令行与路由.md](docs/命令行与路由.md)。

## 后端模型支持

`POST {api_base}/chat/completions`（OpenAI 兼容）。`[agent]` 里配置 **`api_base`**、**`model`**、**`max_tokens`**（嵌入默认 **4096**）、**`llm_http_auth_mode`**；**`bearer`** 时 **`API_KEY`** 走环境变量，**勿**写入仓库配置（[docs/配置说明.md](docs/配置说明.md)）。

| 场景 | 配置要点 |
| --- | --- |
| **DeepSeek** | `api_base`：`https://api.deepseek.com/v1`；常用 `model` 见 **`config/llm_vendors.toml`**（`deepseek-v4-flash`、`deepseek-v4-pro`、`deepseek-v4-flash-vision-exp` 等）。会话附图仍是 `/uploads/`；出站仅 **vision-exp** 会打成 `data:`。[官网](https://platform.deepseek.com/) · [API](https://api-docs.deepseek.com/api/create-chat-completion) |
| **MiniMax** | `api_base`：`https://api.minimaxi.com/v1`（国际站 `https://api.minimax.io/v1`）；`model` 如 `MiniMax-M3`。[配置说明](docs/配置说明.md) · [厂商 OpenAI 兼容](https://platform.minimax.io/docs/api-reference/text-openai-api) |
| **智谱 GLM** | `api_base`：`https://open.bigmodel.cn/api/paas/v4`；`model` 如 `glm-5.3`。[配置说明](docs/配置说明.md) · [GLM-5.3](https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3) |
| **Moonshot Kimi** | `api_base`：`https://api.moonshot.cn/v1`；`model` 如 `kimi-k3`。[配置说明](docs/配置说明.md) · [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) |
| **本地 Ollama 等** | `llm_http_auth_mode = "none"`，`api_base` 如 `http://127.0.0.1:11434/v1`；可不设 `API_KEY`。 |

本机诊断：**`crabmate doctor`**（无需 `API_KEY`）、**`probe`** / **`models`**。各厂商特有选项见 [docs/配置说明.md](docs/配置说明.md)。**厂商能力以供应商文档为准**。

## 部署与安全

- **监听**：默认 **`127.0.0.1`**；监听 **`0.0.0.0`** 须 **`web_api_bearer_token`** 或显式不安全开关。
- **Web API 鉴权**：嵌入默认 **`web_api_require_bearer = false`**——允许无共享密钥启动 **`serve`**；密钥非空时请求须带 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**。可用 **`CM_WEB_API_BEARER_TOKEN`** / **`web_api_bearer_token`** / **`crabmate web-bearer set`** 配置。浏览器须在 **设置 →「Web API 共享密钥」** 保存与服务端相同的值（`localStorage` **`crabmate-api-bearer-token`**），**不要**与模型 **`API_KEY`** 混淆。对外建议 **`web_api_require_bearer = true`**。
- **CORS**：官方壳 Origin 已默认放行；其它浏览器 Origin 用 **`CM_WEB_CORS_ALLOWED_ORIGINS`** 追加。
- **`http_fetch` / `http_request`**：嵌入默认 **`http_fetch_allowed_prefixes = ["*"]`**，任意 **http/https** 不再走前缀审批（仍拒绝 `file:` 等）。多租户或非本机监听请改回具体前缀或 **`[]`**。
- **LLM API Key**：Client 本机存放并经 **`client_llm.api_key`** 发送；进程环境变量 **`API_KEY`** 仍可作为 **`serve`** / 运维回退。
- **个人 VPS（反代 TLS）**：见 [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md)。

详见 [docs/配置说明.md](docs/配置说明.md)。调试与 **`GET /web-ui`** 见 [docs/调试指南.md](docs/调试指南.md)。

## 安装与发行包

| 方式 | 命令 / 说明 |
| --- | --- |
| **安装到 PATH** | **`cargo install crabmate`**（crates.io **稳定版 `0.6.0`**，默认 feature **`server`**；git tag **`v0.6.0`**）。**不**附带 **man**；可手动安装 **[man/crabmate.1](man/crabmate.1)**。 |
| **一键 tar.gz / .deb** | **`make package`**（或 **`./scripts/package-release.sh`**）→ **`dist/`**（二进制、`config/`、man、**`systemd/`**、**`etc/crabmate/`**；**server-only，不附带 UI**）。仅 tar：**`make package-tar`**；仅 deb：**`make package-deb`**（需 **`cargo-deb`**）。 |
| **桌面 / APK** | **仅** Client 仓（[`crabmate-client`](https://github.com/noisystreet/crabmate-client)）。 |
| **同步 man 页** | **`cargo run --features gen-man --bin crabmate-gen-man`**。 |

版本与 semver 面：[docs/design/crates_io_single_package.md](docs/design/crates_io_single_package.md)。兼容矩阵：[docs/design/client_compat_matrix.md](docs/design/client_compat_matrix.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/配置说明.md](docs/配置说明.md) | 环境变量、`CM_*`、Web/TOML 详解 |
| [docs/工具说明.md](docs/工具说明.md) | 内置工具与调用示例 |
| [docs/命令行与路由.md](docs/命令行与路由.md) | 子命令、HTTP 路由、打包 |
| [docs/SSE协议.md](docs/SSE协议.md) | `/chat/stream` 控制面 JSON |
| [docs/开发文档.md](docs/开发文档.md) | 架构概要、主要模块 |
| [docs/测试指南.md](docs/测试指南.md) | 测试、pre-commit、审计命令 |
| [docs/工作流编写教程.md](docs/工作流编写教程.md) | 工作流 YAML/steps 编写与示例 |
| [CHANGELOG.md](CHANGELOG.md) | 发版说明（Keep a Changelog） |

中英文全量对照：[docs/中英文文档对照.md](docs/中英文文档对照.md)。设计文档索引：[docs/design/README.md](docs/design/README.md)。

开发：默认 Cargo feature 为 **`server`**；按需开启 **`fastembed`**、**`project_metrics`**、**`docker_sandbox`**、**`gen-man`**（见根目录 **`Cargo.toml`** **`[features]`** 与 [AGENTS.md](AGENTS.md)）。fmt / clippy / test / pre-commit / SSE / E2E 见 [docs/测试指南.md](docs/测试指南.md)。
