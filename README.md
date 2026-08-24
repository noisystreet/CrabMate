**Languages / 语言:** English (this page) · [中文](README.zh.md)

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

**CrabMate** is a Rust-based AI agent that speaks **OpenAI-compatible** `chat/completions` to backends such as DeepSeek, MiniMax, Zhipu GLM, Moonshot Kimi, and local Ollama.

It ships HTTP **`serve`** (API-only by default) plus ops CLIs. **Official Web UI, Desktop/Android, and remote terminal (`crabmate-tui`)** live in **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** (local checkouts default to sibling `../crabmate-client`; Playwright forwarding honors **`CRABMATE_CLIENT_DIR`**). In-process **`repl` / `chat` / `tui` command entries are removed** (use Client **`crabmate-tui`**; see [ADR](docs/design/client_shell_split.md)).

**Path A (repo split):** this repository maintains **Server** (`serve`, contracts, ops CLI). Official clients are in **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** ([ADR](docs/design/client_shell_split.md)).

## Contents

- [Overview](#overview)
- [Common subcommands](#common-subcommands)
- [Build, run, and packaging](#build-run-and-packaging)
  - [Makefile (recommended)](#makefile-recommended)
  - [Backend](#backend)
  - [Web frontend](#web-frontend)
  - [Official Client (Desktop / Android)](#official-client-desktop-android)
  - [Install and release artifacts](#install-and-release-artifacts)
  - [Maintainer QA](#maintainer-qa)
- [Documentation index](#documentation-index)
- [Backend models](#backend-models)
- [Environment variables](#environment-variables)
- [Deployment and security](#deployment-and-security)
- [Project structure](#project-structure)

## Overview

- **Chat and tools**: OpenAI-compatible `chat/completions`; built-in workspace files, **`run_command`** (allowlist; defaults include **`bash`/`sh`**—glob/`$VAR`/`~` run via **`bash -c`** on the joined script; Web re-approves standalone `&&`/`|` even if bash is allowlisted; approval shows that script; argv outside the workspace or path-traversal-shaped `..` defaults to approval via **`allow_external_path_with_approval`**—git `A..B` is not treated as traversal), HTTP, **web search** (default **worbrow** local browser, no API key; optional Brave/Tavily), workspace **code search** (keyword + optional semantic/embeddings). Full list: [docs/en/TOOLS.md](docs/en/TOOLS.md). Subprocess tool output is truncated by **`command_max_output_len`** (embedded default **512KiB**); see **`config/tools.toml`** and [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).
- **Web UI (Client)**: built and shipped from **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**; this repo’s **`serve` defaults to API-only**; host a SPA with **`--with-web`** plus **`CM_WEB_STATIC_DIR`** (or probed Client `frontend/dist`). Sessions, workspace picker / project pool, editor mode, PR views, terminal-style chat stream, Ask/Plan/Act, and settings—see Client README and [docs/en/CLI.md](docs/en/CLI.md). Tools and **`@relative-path`** apply only after a workspace is selected. Assistant Markdown can show workspace plots with **`![alt](relative/plot.png)`** (Client loads **`GET /workspace/file/raw`** with API auth; png/jpg/jpeg/webp/gif only). Client **Save to this device** uses **`GET /workspace/file/download`** (any type, 16 MiB). Folder zip uses **`GET /workspace/dir/archive`**. Rename/move a file with **`POST /workspace/file/move`**. Client can drop local files onto the workspace tree via **`PUT /workspace/file/raw`** (raw bytes, 16 MiB).
- **Terminal**: Official remote client is **`crabmate-tui`** in **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** (HTTP/SSE to **`serve`**; LLM keys stay on the client). In-process **`repl` / `chat` / `tui` are hard-deleted** (D2.2—[`docs/design/client_shell_split.md`](docs/design/client_shell_split.md) §2.5). **`serve`** is HTTP API-only by default (optional **`--with-web`**). Streaming **SSE**: [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md).
- **Sessions and export**: by default **Web `serve`** persists under **`<workspace>/.crabmate/conversations.db`**; clear **`conversation_store_sqlite_path`** to disable. Web or CLI **`save-session`** (alias **`export-session`**) → JSON/Markdown; shape in [docs/en/CLI.md](docs/en/CLI.md).
- **Advanced (skip by default)**: staged-plan timeline, clarification UI, **`thinking_trace`**, long-term memory, living docs, **MCP**, workspace **`plugins/*.json`**: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md), [docs/en/TOOLS.md](docs/en/TOOLS.md).

## Common subcommands

With no subcommand, clap requires an explicit command (e.g. **`serve`**). Prefer **`serve`** + Client **`crabmate-tui`**. Common globals: **`--config`**, **`--workspace`**, **`--no-tools`**, **`--llm-context-tokens`**, **`--log`** (see **`crabmate --help`**).

The embedded context-history ceiling is **64 messages** (`max_message_history` / `CM_MAX_MESSAGE_HISTORY`) so tool-heavy turns are less likely to trim early. It remains a message-count safety limit rather than exact token accounting; see [Configuration](docs/en/CONFIGURATION.md).

| Subcommand | Summary |
| --- | --- |
| **`serve`** | HTTP API (**API-only by default**; no SPA). Host UI with **`--with-web`** and **`CM_WEB_STATIC_DIR`** (or probed Client/`frontend/dist` / install path). Default port **8080**, bind **127.0.0.1**. |
| **`doctor`** | One-page local diagnostics (**no** `API_KEY`). |
| **`config`** | Load config and self-check (e.g. **`--dry-run`**). |
| **`models`** / **`probe`** | Probe **`GET …/models`** on **`api_base`**; **`bearer`** usually needs env **`API_KEY`**. |
| **`save-session`** | Export session file to **`<workspace>/.crabmate/exports/`** (alias **`export-session`**). |
| **`bench`** | Batch evaluation (JSONL): [benchmark/README.md](benchmark/README.md), [docs/基准测试规划.md](docs/基准测试规划.md). |
| **`mcp`** | **`mcp list`** / **`mcp list --probe`**; **`mcp serve`** exposes built-in tools over stdio (**no** transport auth). |
| **`plugin`** | **`init`** / **`list`** / **`validate`**: workspace **`plugins/*.json`** (**`dyn__`** prefix). |
| **`workflow`** | **`compile`** / **`validate`** / **`run`**: workspace YAML/Markdown workflows (**no** `API_KEY`); [docs/工作流编写教程.md](docs/工作流编写教程.md). |
| **`tool-replay`** | Export or replay tool fixtures (**no** `API_KEY`; trusted workspace only). |

Full flags, HTTP routes, **`man crabmate`**: [docs/en/CLI.md](docs/en/CLI.md).

## Build, run, and packaging

**Prerequisites**: **Rust 1.85+** (edition 2024). Official UI is in the Client repo (Trunk / wasm32). More: [AGENTS.md](AGENTS.md).

### Makefile (recommended)

```bash
make help              # list targets
make all / all-dev     # backend-release / backend
make backend           # cargo build -p crabmate
make package           # server-only tar.gz + optional .deb → dist/ (no UI)
make clean             # clean target and dist/
```

UI: `cd ../crabmate-client && make frontend` (clone [crabmate-client](https://github.com/noisystreet/crabmate-client) as a sibling first). **`make package`** / **`package-tar`** / **`package-deb`** are **server-only** (API-only by default; use **`--with-web`** + **`CM_WEB_STATIC_DIR`** to host SPA). Desktop / Android: **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**.

### Backend

```bash
# Debug
cargo build
./target/debug/crabmate serve            # API-only by default
./target/debug/crabmate serve --with-web # host SPA (needs CM_WEB_STATIC_DIR or probed dist)
# or: API_KEY=… ./target/debug/crabmate serve

# Release
cargo build --release
./target/release/crabmate serve

# Optional: Ubuntu 24.04 toolchain image (dev + `make package`; glibc 2.39; not a runtime)
# docker build -t crabmate-dev .          # add --network=host only if DNS fails
# docker run --rm -it -v "$PWD":/workspace -w /workspace crabmate-dev
# make package-docker                     # → dist/*.tar.gz and dist/*.deb on the host
```

**`serve`** Web API auth (**`CM_WEB_API_BEARER_TOKEN`**, etc.): **[Deployment and security](#deployment-and-security)**. Cloud **`API_KEY`**: **[Environment variables](#environment-variables)** (or Client Web Settings / `client_llm` on requests).

### Web frontend

Official UI source: **[`frontend/`](https://github.com/noisystreet/crabmate-client/tree/main/frontend)** in [crabmate-client](https://github.com/noisystreet/crabmate-client) (path A Phase 4.2). Local default: sibling `../crabmate-client`.

```bash
cd ../crabmate-client && make frontend
export CM_WEB_STATIC_DIR="$PWD/frontend/dist"
cd ../crabmate_agent && cargo run -- serve --with-web
```

API-only (default): `serve`. UI pointers: [`docs/frontend/`](docs/frontend/).

### Official Client (Desktop / Android)

> **Canonical repo**: **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)** (path A; [ADR](docs/design/client_shell_split.md); local sibling `../crabmate-client`).  
> This repo **removed** `desktop-tauri/` / `mobile-tauri/` / `crates/crabmate-connect` (Phase 4.1).

The shell **does not** spawn `serve`: start **`crabmate serve`**, then enter URL + Web API Bearer on the Client connect page.

```bash
cd ../crabmate-client
make desktop-release    # Linux .deb (no serve sidecar)
# or make apk / cargo tauri dev — see Client README
```

Compat matrix: [`docs/design/client_compat_matrix.md`](docs/design/client_compat_matrix.md).

### Install and release artifacts

| Method | Command / notes |
| --- | --- |
| **Install to PATH** | **`cargo install crabmate`** (crates.io **stable `0.4.0`**, default feature **`server`**). Tree / GitHub pre-release **`0.5.0-alpha.3`** (`v0.5.0-alpha.3`): **`cargo install --path .`** or the Release tarball/`.deb`. Does **not** ship **man**; install **[man/crabmate.1](man/crabmate.1)** manually if needed. |
| **Tarball / .deb** | **`make package`** (or **`./scripts/package-release.sh --skip-frontend`**) → **`dist/`** (binary, `config/`, man, **`systemd/`**, **`etc/crabmate/`**; **no UI by default**). Tar only: **`make package-tar`**; deb only: **`make package-deb`** (needs **`cargo-deb`**). Optional **`--frontend-dist`** is script-only. |
| **Debian (.deb)** | **`make package-deb`** / **`cargo deb`** (UI not required); under **`dist/`** or **`target/debian/`**. Installs **`crabmate.service`** (**127.0.0.1:8080**, API-only by default; add **`--with-web`** + **`CM_WEB_STATIC_DIR`** for UI). Desktop shell `.deb`: Client repo. Details: [docs/en/CLI.md](docs/en/CLI.md). |
| **Desktop / APK** | **Only** the Client repo ([`crabmate-client`](https://github.com/noisystreet/crabmate-client)). |
| **Regenerate man** | **`cargo run --features gen-man --bin crabmate-gen-man`**. |

### Maintainer QA

- **Cargo features**: default **`server`** (includes **`protocol`**, **`web`**, **`mcp`**); opt-in **`fastembed`**, **`project_metrics`**, **`docker_sandbox`**, **`gen-man`**. Examples: `cargo build --features fastembed`, `--features project_metrics`, or `--all-features`. In-process **`repl`/`tui` features removed** (D2.2; use Client **`crabmate-tui`**). See root **`Cargo.toml`** **`[features]`** and **`AGENTS.md`**.
- **fmt / clippy / test, pre-commit, SSE, E2E**: [docs/en/TESTING.md](docs/en/TESTING.md) (includes **`./scripts/check-sse-protocol.sh`**). CI also runs **`make package`** (server-only tar.gz + `.deb` smoke).

## Documentation index

| Document | Contents | 中文 |
| --- | --- | --- |
| [CHANGELOG.md](CHANGELOG.md) | Release notes (Keep a Changelog) | — |
| [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md) | Architecture overview, main modules, data flow | [zh](docs/开发文档.md) |
| [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md) | Env vars, `CM_*`, Web/TOML | [zh](docs/配置说明.md) |
| [docs/en/TOOLS.md](docs/en/TOOLS.md) | Built-in tools and examples | [zh](docs/工具说明.md) |
| [docs/工作流编写教程.md](docs/工作流编写教程.md) | Workflow YAML/steps (Chinese) | — |
| [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md) | `/chat/stream` control JSON | [zh](docs/SSE协议.md) |
| [docs/en/CLI.md](docs/en/CLI.md) | Subcommands, HTTP routes, packaging | [zh](docs/命令行与路由.md) |
| [docs/en/CLI_CONTRACT.md](docs/en/CLI_CONTRACT.md) | `chat` exit codes, **`--output json`** | [zh](docs/命令行契约.md) |
| [docs/en/DEBUG.md](docs/en/DEBUG.md) | Logging, `doctor`, `GET /web-ui`, … | [zh](docs/调试指南.md) |
| [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) | Personal VPS: loopback `serve` + TLS + Bearer (Chinese) | — |
| [docs/en/TESTING.md](docs/en/TESTING.md) | Tests, pre-commit, audits | [zh](docs/测试指南.md) |
| [docs/design/client_shell_split.md](docs/design/client_shell_split.md) | Official Client split (path A) | — |
| [docs/design/frontend_migrate_plan.md](docs/design/frontend_migrate_plan.md) | Phase 4.2 UI migrate plan | — |
| [docs/design/client_compat_matrix.md](docs/design/client_compat_matrix.md) | Server ↔ protocol ↔ Client compat | — |
| [docs/基准测试规划.md](docs/基准测试规划.md) | **`bench`** roadmap | — |
| [docs/BENCHMARK_RESULTS.md](docs/BENCHMARK_RESULTS.md) | Recorded bench scores (no secrets) | — |
| [benchmark/README.md](benchmark/README.md) | HumanEval convert/run/smoke | — |

**More**: backlog, roadmap, frontend drafts—under **`docs/`** ([docs/中英文文档对照.md](docs/中英文文档对照.md)).

**Maintenance**: keep user-visible docs in sync; conventions in [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md).

## Backend models

`POST {api_base}/chat/completions` (OpenAI-compatible). Under **`[agent]`** set **`api_base`**, **`model`**, **`max_tokens`** (embedded default **4096**), **`llm_http_auth_mode`**; with **`bearer`**, use env **`API_KEY`**—**never** commit real keys.

| Scenario | Notes |
| --- | --- |
| **DeepSeek** | `api_base`: `https://api.deepseek.com/v1`; common `model` ids in **`config/llm_vendors.toml`** (`deepseek-v4-flash`, `deepseek-v4-pro`, `deepseek-v4-flash-vision-exp`, …). Chat attachments stay as `/uploads/` in session; only **vision-exp** inlines them as `data:` on the wire. [Platform](https://platform.deepseek.com/) · [API](https://api-docs.deepseek.com/api/create-chat-completion) |
| **MiniMax** | `api_base`: `https://api.minimaxi.com/v1` (intl. `https://api.minimax.io/v1`); `model` e.g. `MiniMax-M3`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [Vendor OpenAI-compatible API](https://platform.minimax.io/docs/api-reference/text-openai-api) |
| **Zhipu GLM** | `api_base`: `https://open.bigmodel.cn/api/paas/v4`; `model` e.g. `glm-5.3`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [GLM-5.3](https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3) |
| **Moonshot Kimi** | `api_base`: `https://api.moonshot.cn/v1`; `model` e.g. `kimi-k3`. [CONFIGURATION](docs/en/CONFIGURATION.md) · [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) |
| **Local Ollama** | `llm_http_auth_mode = "none"`; `api_base` e.g. `http://127.0.0.1:11434/v1`; **`API_KEY`** optional. |

Local checks: **`crabmate doctor`** (no `API_KEY`), **`probe`** / **`models`**. Vendor knobs: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). **Vendor behavior is defined by provider docs.**

## Environment variables

| Variable | Role |
| --- | --- |
| **`API_KEY`** | Cloud bearer (**`llm_http_auth_mode=bearer`**); optional process fallback for **`serve`** / **`models`** / **`probe`**. Official Client dialogue sends **`client_llm.api_key`** (keychain on the client). |
| **`CM_API_BASE`** / **`CM_MODEL`** | Override gateway and model from config. |
| **`CM_WEB_API_BEARER_TOKEN`** | Protects Web APIs (with **`web_api_require_bearer`**); [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). |
| **`CM_WEB_CORS_ALLOWED_ORIGINS`** | Extra Origin allowlist (comma-separated); **unset** already allows official shell Origins (`tauri://localhost`, `http://tauri.localhost`). Explicit empty disables CORS. Static browser UI: add its Origin; see Settings **API base** (`localStorage` **`crabmate-api-base-url`**). |
| **`CM_WEB_STATIC_DIR`** | Override static root when **`serve --with-web`** (Client `frontend/dist` / install path; SPA off by default). |
| **`CM_DESKTOP_SUGGESTED_URL`** | Optional connect-page suggested `serve` URL (default `http://127.0.0.1:8080/`). |
| **`CM_DESKTOP_SERVE_URL`** | Required when skipping connect page (with **`CM_DESKTOP_SKIP_CONNECT`** / **`CM_E2E_FIXTURES`**). |

Other **`CM_*`** (skills, staged planning, etc.): [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).

## Deployment and security

- **Listen**: default **`127.0.0.1`**; **`0.0.0.0`** needs **`web_api_bearer_token`** or an explicit insecure switch ([docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md)).
- **`http_fetch` / `http_request`**: embedded default **`http_fetch_allowed_prefixes = ["*"]`** — any **http/https** URL skips prefix approval (still rejects `file:` etc.). On multi-tenant or non-loopback listen, set concrete prefixes or **`[]` / empty `CM_HTTP_FETCH_ALLOWED_PREFIXES`** to override the embed ([docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md)).
- **LLM API Key**: Client stores keys locally and sends **`client_llm.api_key`**. Process env **`API_KEY`** remains an optional **`serve`** / ops fallback.
- **Web API**: embedded default **`web_api_require_bearer = false`**—**`serve`** may start without a shared secret; with **`true`**, require non-empty **`CM_WEB_API_BEARER_TOKEN`** (or TOML / **`crabmate web-bearer set`**). When the token is set, send **`Authorization: Bearer …`** or **`X-API-Key: …`**. Browsers must save the **same** value under **Settings → Web API shared secret** (`localStorage` **`crabmate-api-bearer-token`**)—**not** the LLM **`API_KEY`**. Cross-origin static UI: set **API base**; official shell Origins are allowed by default—add **`CM_WEB_CORS_ALLOWED_ORIGINS`** only for extra browser Origins. Smoke: **`docs/design/client_turn_smoke_runbook.md`** §9. Temporary local skip: unset the secret and bind **`127.0.0.1`**, or clear it and set **`CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`** before **`0.0.0.0`**. Prefer **`web_api_require_bearer = true`** on exposed networks. Details: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).
- **Other**: Web **Settings → Save all** persists via **`/user-data`**; workspace must stay under allowed roots. Debug / **`GET /web-ui`**: [docs/en/DEBUG.md](docs/en/DEBUG.md).
- **Personal VPS (TLS reverse proxy)**: [docs/个人VPS部署指南.md](docs/个人VPS部署指南.md) (Chinese; **`127.0.0.1` + Bearer + Caddy/Nginx**).

## Project structure

Architecture overview: [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md). **`GET /status`** for full runtime status; Web shell uses **`GET /status?view=shell`**. More: [docs/en/DEBUG.md](docs/en/DEBUG.md).

- **Single crate**: crates.io **stable** is **`0.4.0`** ([crates.io/crates/crabmate](https://crates.io/crates/crabmate), default **`server`**). **`cargo install crabmate`** still installs that. This tree is **`0.5.0-alpha.3`** (git tag **`v0.5.0-alpha.3`**, GitHub **prerelease** artifacts). Official Client pins **`version = "0.4.0", default-features = false, features = ["protocol"]`** (`crabmate::cm_sse_protocol`, `cm_types`, … — not `types`/`sse` aliases).
- **Semver surface**: `protocol` = the six `cm_*` contract modules. `server` promises the composition module *names* (`agent` / `config` / `llm` / `sse` / `types`) and explicit root `pub use`s (`run`, `run_agent_turn`, `build_tools*`, …). `#[doc(hidden)]` modules and paths such as `agent::agent_turn` are **not** a stable SDK. Details: [docs/design/crates_io_single_package.md](docs/design/crates_io_single_package.md) §2.4.
