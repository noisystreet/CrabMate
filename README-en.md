**Languages / 语言:** English (this page) · [中文](README.md)

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

**CrabMate** is a Rust-based AI agent that speaks **OpenAI-compatible** `chat/completions` to backends such as DeepSeek, MiniMax, Zhipu GLM, Moonshot Kimi, and local Ollama.

It includes **function calling**, workspace command and file tools, plus a **Web UI** and **CLI**.

## Contents

- [Overview](#overview)
- [Common subcommands](#common-subcommands)
  - [TUI (full-screen terminal)](#tui-full-screen-terminal)
- [Build, run, and packaging](#build-run-and-packaging)
  - [Backend](#backend)
  - [Web frontend](#web-frontend)
  - [Desktop Tauri](#desktop-tauri)
  - [Install and release artifacts](#install-and-release-artifacts)
  - [Maintainer QA](#maintainer-qa)
- [Documentation index](#documentation-index)
- [Backend models](#backend-models)
- [Environment variables](#environment-variables)
- [Deployment and security](#deployment-and-security)
- [Project structure](#project-structure)

## Overview

- **Chat and tools**: OpenAI-compatible `chat/completions`; built-in workspace files, **`run_command`** (allowlist; defaults include **`bash`/`sh`** for **`bash -c`/`sh -c`** compound scripts), HTTP/search, workspace **code search** (keyword + optional semantic/embeddings); full list in [docs/en/TOOLS.md](docs/en/TOOLS.md).
- **Web UI**: sidebar sessions and workspace; tools and **`@relative-path`** only apply after you **pick a workspace**; assistant **Markdown**; **`@` references**, image attachments (vision-capable models), session export, etc. Fullscreen **Settings → Session** toggles whether **this server process** persists Web transcripts to SQLite (subject to **`conversation_store_sqlite_path`**; **restarting `serve`** still follows config files). Routes and behavior: [docs/en/CLI.md](docs/en/CLI.md).
- **Terminal**: **`repl`** (interactive), **`chat`** (one-shot), **`serve`** (HTTP + static UI), **`tui`** (experimental **full-screen**, real TTY required—see below). Streaming **SSE**, tool approval/cancel: [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md).
- **Sessions and export**: by default **Web `serve`** (and **`tui`** when configured with the same path) persist transcripts under **`<workspace>/.crabmate/conversations.db`** so a **`conversation_id`** survives **`serve` restarts**; clear **`conversation_store_sqlite_path`** in config to disable server-side persistence. Web or CLI **`save-session`** (alias **`export-session`**) to JSON/Markdown; shape in [docs/en/CLI.md](docs/en/CLI.md).
- **Advanced (skip by default)**: staged-plan timeline, clarification UI, debug **`thinking_trace`**, long-term memory, living docs, **MCP**, workspace **`plugins/*.json`**, etc.: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md), [docs/en/TOOLS.md](docs/en/TOOLS.md).

## Common subcommands

With no subcommand, **`repl`** runs. Common globals: **`--config`**, **`--workspace`**, **`--no-tools`**, **`--agent-role`**, **`--llm-context-tokens`**, **`--log`** (see **`crabmate --help`**).

| Subcommand | Summary |
| --- | --- |
| **`serve`** | HTTP API + **`frontend/dist`** Web UI (default **8080**, bind **127.0.0.1**). |
| **`repl`** | Interactive terminal; **`/`** commands and **`/api-key set`**: [docs/en/CLI.md](docs/en/CLI.md). |
| **`chat`** | One-shot then exit (**`--query`** / **`--stdin`** / files); **`--output json`**: [docs/en/CLI_CONTRACT.md](docs/en/CLI_CONTRACT.md). |
| **`tui`** | Experimental **full-screen** terminal UI; needs an **interactive TTY** (otherwise use **`repl`** / **`chat`**). Summary: **[TUI (full-screen terminal)](#tui-full-screen-terminal)**. |
| **`doctor`** | One-page local diagnostics (**no** `API_KEY`). |
| **`config`** | Load config and self-check (e.g. **`--dry-run`**). |
| **`models`** / **`probe`** | Probe **`GET …/models`** on **`api_base`**; **`bearer`** usually needs env **`API_KEY`**. |
| **`save-session`** | Export session file to **`<workspace>/.crabmate/exports/`** (alias **`export-session`**). |
| **`bench`** | Batch evaluation (JSONL): [benchmark/README.md](benchmark/README.md), [docs/基准测试规划.md](docs/基准测试规划.md). |
| **`mcp`** | **`mcp list`** / **`mcp list --probe`**; **`mcp serve`** exposes built-in tools over stdio (**no** transport auth). |
| **`plugin`** | **`init`** / **`list`** / **`validate`**: workspace **`plugins/*.json`** dynamic tools (**`dyn__`** prefix). |
| **`tool-replay`** | Export or replay tool fixtures (**no** `API_KEY`; trusted workspace only). |

Full flags, HTTP routes, **`man crabmate`**: [docs/en/CLI.md](docs/en/CLI.md).

### TUI (full-screen terminal)

**`crabmate tui`** is an experimental **full-screen** UI sharing the same agent/tool stack as **`repl`**; use it when you want workspace / task / change previews in the terminal without a browser.

- **Environment**: real **TTY** required; otherwise use **`repl`** / **`chat`**.
- **Interaction**: **Enter** sends from the composer; with focus on the right **Workspace** pane, **Enter** opens path browse (same rules as Web **`/workspace`** and REPL **`/workspace`**). **`q`** / **Ctrl+C** to quit. **`/api-key`** and other **`/`** commands match **`repl`**.
- **Streaming**: assistant stream is not painted on **stdout**; see **`--no-stream`** in **`crabmate tui --help`**.
- **More**: optional SQLite multi-session (**`/conv`**, **`/branch`**), clarification flows, **`CM_TUI_CONVERSATION_ID`**, session snapshot file—**[docs/en/CLI.md](docs/en/CLI.md)**.

## Build, run, and packaging

**Prerequisites**: **Rust 1.85+** (edition 2024); for Web, install [**Trunk**](https://trunkrs.dev/) and target **`wasm32-unknown-unknown`** (**`rustup target add wasm32-unknown-unknown`**). More: [AGENTS.md](AGENTS.md).

### Backend

```bash
# Debug binary
cargo build
./target/debug/crabmate serve    # or repl / chat …

# Release binary
cargo build --release
./target/release/crabmate serve
```

**`serve`** Web API auth (**`CM_WEB_API_BEARER_TOKEN`**, etc.): **[Deployment and security](#deployment-and-security)**. Cloud **`API_KEY`**: **[Environment variables](#environment-variables)** (or Web Settings, REPL **`/api-key set`**).

### Web frontend

**`crabmate serve`** serves static files from **`frontend/dist`**; no separate frontend process.

```bash
cd frontend
trunk build              # dev; release: trunk build --release
```

Then from the repo root: **`crabmate serve`** (or **`cargo run -- serve`**). Details: **`frontend/README.md`**.

### Desktop Tauri

Tree: **`desktop-tauri/`**. The **WebView** loads **`serve`** spawned as **`--host 127.0.0.1 --port 0 --desktop-ready-json`**; the shell parses the **`web_ready`** JSON on stdout (see [**desktop-tauri/README.md**](desktop-tauri/README.md)). If **`crabmate`** is not on **`PATH`**, set **`CM_DESKTOP_BACKEND_BIN`** to the built binary.

```bash
cargo build
cd frontend && trunk build && cd ..
cargo install tauri-cli --version "^2"   # once
cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/absolute/path/to/target/debug/crabmate cargo tauri dev
```

Release: **`cargo tauri build`**. Proxies and troubleshooting: [**desktop-tauri/DEVELOPMENT.md**](desktop-tauri/DEVELOPMENT.md).

### Install and release artifacts

| Method | Command / notes |
| --- | --- |
| **Install to PATH** | **`cargo install --path .`** (**does not** ship **man**; install **[man/crabmate.1](man/crabmate.1)** manually if needed). |
| **Tarball** | **`./scripts/package-release.sh`** → **`dist/crabmate_<version>_<os>_<arch>.tar.gz`** (binary, `config/`, `frontend/dist`, man); with **`cargo-deb`**, may also collect **`.deb`**. |
| **Debian (.deb)** | After **`trunk build --release`** in **`frontend`**, **`cargo deb`** → **`target/debian/`**. Details: [docs/en/CLI.md](docs/en/CLI.md). |
| **Desktop (Tauri)** | Desktop bundles (current config defaults to **Linux `.deb`**—see **`bundle.targets`** in **`desktop-tauri/src-tauri/tauri.conf.json`**); steps below. |
| **Regenerate man** | **`cargo run --bin crabmate-gen-man`**. |

**Tauri desktop packaging (example, repo root):**

```bash
cargo build --release
cd frontend && trunk build --release && cd ..
rm -rf desktop-tauri/dist && cp -r frontend/dist desktop-tauri/dist

cd desktop-tauri/src-tauri
# beforeBuildCommand runs ../scripts/prepare-sidecar.sh; or run manually: bash ../scripts/prepare-sidecar.sh
cargo tauri build
```

**`prepare-sidecar.sh`** copies **`target/release/crabmate`** (or **`CM_DESKTOP_BACKEND_BIN`**) into **`desktop-tauri/binaries/`** as the backend **sidecar**. Bundles usually land under **`desktop-tauri/src-tauri/target/release/bundle/deb/`** (exact names depend on **`productName`** / version). **`bundle.targets`**, **`GDK_BACKEND`**, etc.: [**desktop-tauri/DEVELOPMENT.md**](desktop-tauri/DEVELOPMENT.md).

### Maintainer QA

- **Cargo features / slim binaries**: defaults **`mcp` + `web` + `repl` + `tui`**; **`fastembed`** (local ONNX embeddings) and **`docker_sandbox`** are opt-in. Enable semantic search / vector long-term memory with e.g. `cargo build --features fastembed`. Slim build: `cargo build --no-default-features --features web,repl,tui`. See root **`Cargo.toml`** **`[features]`** and **`AGENTS.md`**.
- **fmt / clippy / test, pre-commit, SSE script, E2E**: **[docs/en/TESTING.md](docs/en/TESTING.md)** (includes **`./scripts/check-sse-protocol.sh`**).

## Documentation index

| Document | Contents | 中文 |
| --- | --- | --- |
| [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md) | Architecture overview, main modules, data flow | [zh](docs/开发文档.md) |
| [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md) | Env vars, `CM_*`, Web/TOML | [zh](docs/配置说明.md) |
| [docs/en/TOOLS.md](docs/en/TOOLS.md) | Built-in tools and examples | [zh](docs/工具说明.md) |
| [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md) | `/chat/stream` control JSON | [zh](docs/SSE协议.md) |
| [docs/en/CLI.md](docs/en/CLI.md) | Subcommands, HTTP routes, deb | [zh](docs/命令行与路由.md) |
| [docs/en/CLI_CONTRACT.md](docs/en/CLI_CONTRACT.md) | `chat` exit codes, **`--output json`** | [zh](docs/命令行契约.md) |
| [docs/en/DEBUG.md](docs/en/DEBUG.md) | Logging, `doctor`, `GET /web-ui`, … | [zh](docs/调试指南.md) |
| [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) | Personal VPS: loopback `serve` + TLS reverse proxy + Bearer (Chinese) | — |
| [docs/en/TESTING.md](docs/en/TESTING.md) | Tests, pre-commit, audits | [zh](docs/测试指南.md) |
| [docs/基准测试规划.md](docs/基准测试规划.md) | **`bench`** roadmap & benchmarks | — |
| [benchmark/README.md](benchmark/README.md) | HumanEval convert/run/smoke | — |

**More**: maintainer backlog, roadmap, frontend architecture drafts, full zh/en map—under **`docs/`** (index: [docs/中英文文档对照.md](docs/中英文文档对照.md)).

**Maintenance**: keep user-visible docs in sync with code; conventions in [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md).

## Backend models

`POST {api_base}/chat/completions` (OpenAI-compatible). Under **`[agent]`** set **`api_base`**, **`model`**, **`llm_http_auth_mode`**; with **`bearer`**, use env **`API_KEY`**—**never** commit real keys in repo config.

| Scenario | Notes |
| --- | --- |
| **DeepSeek** | `api_base`: `https://api.deepseek.com/v1`; `model` e.g. `deepseek-chat` / `deepseek-reasoner`. [Platform](https://platform.deepseek.com/) · [API](https://api-docs.deepseek.com/api/create-chat-completion) |
| **MiniMax** | `api_base`: `https://api.minimaxi.com/v1`; `model` e.g. `MiniMax-M2.7`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [Vendor OpenAI-compatible API](https://platform.minimaxi.com/docs/api-reference/text-openai-api) |
| **Zhipu GLM** | `api_base`: `https://open.bigmodel.cn/api/paas/v4`; `model` e.g. `glm-5`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [GLM-5](https://docs.bigmodel.cn/cn/guide/models/text/glm-5) |
| **Moonshot Kimi** | `api_base`: `https://api.moonshot.cn/v1`; `model` e.g. `kimi-k2.5`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) |
| **Local Ollama** | `llm_http_auth_mode = "none"`; `api_base` e.g. `http://127.0.0.1:11434/v1`; **`API_KEY`** optional. |

Local checks: **`crabmate doctor`** (no `API_KEY`), **`probe`** / **`models`**. Vendor-specific knobs (thinking, temperature caps, …): [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). **Vendor behavior is defined by provider docs.**

## Environment variables

| Variable | Role |
| --- | --- |
| **`API_KEY`** | Cloud bearer token (**`llm_http_auth_mode=bearer`**); `serve` / `repl` / `chat` can start first, then set via UI or **`/api-key`**. |
| **`CM_API_BASE`** / **`CM_MODEL`** | Override gateway and model from config. |
| **`CM_WEB_API_BEARER_TOKEN`** | Protects Web APIs (**`web_api_require_bearer`**); see [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). |

Other **`CM_*`** (including **`CM_TUI_CONVERSATION_ID`**, skills, staged planning, …): [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).

## Deployment and security

- **Listen**: default **`127.0.0.1`**; **`0.0.0.0`** needs **`web_api_bearer_token`** or an explicit insecure switch ([docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md)).
- **Web API**: embedded default **`web_api_require_bearer = false`**—**`serve`** may start without a shared secret; with **`true`**, a non-empty **`CM_WEB_API_BEARER_TOKEN`** (or TOML **`web_api_bearer_token`**) is required before start. When the token is non-empty, the Bearer layer is mounted; send **`Authorization: Bearer …`** or **`X-API-Key: …`**. The UI may store **`localStorage["crabmate-api-bearer-token"]`**. For exposed or untrusted networks, prefer **`web_api_require_bearer = true`** plus a configured secret.
- **Other**: Web sidebar **Settings** needs **Save all** to persist in the browser; workspace must stay under allowed roots (path checks: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md)). Debug env vars and **`GET /web-ui`**: [docs/en/DEBUG.md](docs/en/DEBUG.md).
- **Personal VPS (reverse-proxy TLS)**: walkthrough (Chinese) in [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) (**`127.0.0.1` + `CM_WEB_API_BEARER_TOKEN` + Caddy/Nginx**).

## Project structure

Layering and main modules: [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md). **`GET /status`** and debugging: [docs/en/DEBUG.md](docs/en/DEBUG.md).

- **Workspace crates**: `crates/crabmate-sse-protocol` (SSE control-plane contract); **`crates/crabmate-im-bridge`** (optional **IM bridge**: Feishu webhook → **`POST /chat`** → reply). See [docs/design/feishu_bridge_mvp.md](docs/design/feishu_bridge_mvp.md).
