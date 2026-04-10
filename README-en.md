**Languages / 语言:** English (this page) · [中文](README.md)

# CrabMate

<p align="center">
  <img src="crabmate.svg" alt="CrabMate Logo" width="240" />
</p>

**CrabMate** is a Rust-based AI agent that uses **OpenAI-compatible** `chat/completions` against backends such as DeepSeek, MiniMax, Zhipu GLM, Moonshot Kimi, and local Ollama.

It ships **function calling**, workspace commands and file tools, plus a **Web UI** and **CLI**.

## Contents

- [Overview](#overview)
- [Documentation index](#documentation-index)
- [Backend models](#backend-models)
- [Environment and quick start](#environment-and-quick-start)
- [Build and packaging](#build-and-packaging)
- [Deployment and security](#deployment-and-security)
- [Project structure](#project-structure)
- [References](#references)

## Overview

- **Chat and models**: OpenAI-compatible `chat/completions`; gateway and model wiring live in config and in **Backend models** below.

- **Built-in tools** (**function calling**): workspace files, **`run_command`** (allowlist), HTTP/search, formatting, dependency graphs and coverage, container helpers; stacks include **Rust / Python / JS·TS / Go / JVM / C·C++** and **GitHub `gh_*`**. Full list and JSON examples: [docs/en/TOOLS.md](docs/en/TOOLS.md).

- **CLI**: **`crabmate repl`** / **`chat`** / **`serve`** (same agent/tools as the Web UI). Details: **[CLI](#cli)** and [docs/en/CLI.md](docs/en/CLI.md).

- **Web UI**: DeepSeek-style layout; assistant replies as **Markdown**; sidebar sessions (export, filter, search), workspace tree and change preview, tasks and context status, multi-select / retry / branch on messages; **multi-role** and more in [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).

- **Project profile**: sidebar summary and optional first-turn injection; models can use **`repo_overview_sweep`** ([docs/en/TOOLS.md](docs/en/TOOLS.md)).

- **OpenAPI**: **`GET /openapi.json`**; streaming control plane is defined in [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md) (including **`client_sse_protocol`** negotiation).

- **Streaming and approval**: Web **SSE** + **`POST /chat/approval`**; CLI terminal approval; cancel codes etc. in [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md) and [docs/en/CLI.md](docs/en/CLI.md) § CLI vs Web.

- **Sessions and export**: optional Web SQLite persistence, export JSON/Markdown; CLI **`save-session`** / **`tool-replay`**, etc. Workspace changelist injection and long-term memory: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).

- **Optional**: in-process tool stats (**`agent_tool_stats_enabled`**); **MCP stdio** (**`mcp_enabled`** + **`mcp_command`**, `crabmate mcp list`). See [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md).

## Documentation index

| Document | Contents | 中文 |
| --- | --- | --- |
| [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md) | Architecture, module index, protocols | [zh](docs/DEVELOPMENT.md) |
| [docs/en/TOOLS.md](docs/en/TOOLS.md) | Built-in tools and JSON examples | [zh](docs/TOOLS.md) |
| [docs/en/SSE_PROTOCOL.md](docs/en/SSE_PROTOCOL.md) | `/chat/stream` control-plane JSON | [zh](docs/SSE_PROTOCOL.md) |
| [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md) | Env vars, `AGENT_*`, planning/context | [zh](docs/CONFIGURATION.md) |
| [docs/en/CLI.md](docs/en/CLI.md) | Subcommands, HTTP routes, `.deb` | [zh](docs/CLI.md) |
| [docs/en/CLI_CONTRACT.md](docs/en/CLI_CONTRACT.md) | `chat` exit codes, `--output json`, SSE cross-ref | [zh](docs/CLI_CONTRACT.md) |
| [docs/en/TODOLIST.md](docs/en/TODOLIST.md) | Open work P0–P5 + by-module | [zh](docs/TODOLIST.md) |
| [docs/en/CODEBASE_INDEX_PLAN.md](docs/en/CODEBASE_INDEX_PLAN.md) | Unified codebase index + incremental cache | [zh](docs/CODEBASE_INDEX_PLAN.md) |

**Maintenance**: user-visible changes should stay in sync with README and related docs; see [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md) § TODOLIST and documentation conventions.

## Backend models

`POST {api_base}/chat/completions` (OpenAI-compatible). Under **`[agent]`** set **`api_base`**, **`model`**, **`llm_http_auth_mode`**; with **`bearer`**, use env **`API_KEY`**—**do not** commit real keys in repo config.

| Scenario | Notes |
| --- | --- |
| **DeepSeek** | `api_base`: `https://api.deepseek.com/v1`; `model` e.g. `deepseek-chat` / `deepseek-reasoner`. See [platform](https://platform.deepseek.com/) and [API docs](https://api-docs.deepseek.com/api/create-chat-completion). |
| **MiniMax** | `api_base`: `https://api.minimaxi.com/v1`; `model` e.g. `MiniMax-M2.7`. System-role folding, `llm_reasoning_split` defaults, etc.: [CONFIGURATION § MiniMax](docs/en/CONFIGURATION.md) and [vendor OpenAI-compatible docs](https://platform.minimaxi.com/docs/api-reference/text-openai-api). |
| **Zhipu GLM** | `api_base`: `https://open.bigmodel.cn/api/paas/v4`; `model` e.g. `glm-5`. Optional `llm_bigmodel_thinking`. [CONFIGURATION](docs/en/CONFIGURATION.md), [GLM-5](https://docs.bigmodel.cn/cn/guide/models/text/glm-5). |
| **Moonshot Kimi** | `api_base`: `https://api.moonshot.cn/v1`; `model` e.g. `kimi-k2.5`. Temperature coercion, `llm_kimi_thinking_disabled`, etc.: [CONFIGURATION](docs/en/CONFIGURATION.md), [Kimi API](https://platform.moonshot.cn/docs/api/chat). |
| **Local Ollama** | `llm_http_auth_mode = "none"`; `api_base` e.g. `http://127.0.0.1:11434/v1`; `API_KEY` optional. |

Local checks: **`crabmate doctor`** (no `API_KEY`), **`probe`** / **`models`**. Full **`AGENT_*`** and hot reload: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). **Vendor capabilities are defined by each provider’s API docs.**

## Environment and quick start

- **Rust**: 1.85+ (edition 2024, see [AGENTS.md](AGENTS.md))

- **Docker dev image** (optional): root [Dockerfile](Dockerfile) (Ubuntu 24.04, Rust + trunk, etc.). `docker build -t crabmate-dev .` then `docker run --rm -it -v "$(pwd)":/workspace -w /workspace crabmate-dev`; UID/GID via `--build-arg DEV_UID` / `DEV_GID`. **No** pre-commit / Node inside.

- **Environment**: **`API_KEY`** when using bearer (`serve` / `repl` / `chat` can start first; set the key in Web Settings or REPL **`/api-key set`** before chatting); **`models` / `probe`** usually still need `API_KEY` in the environment under bearer. **`AGENT_API_BASE`** / **`AGENT_MODEL`** override config. Staged planning: optional **`AGENT_STAGED_PLAN_TWO_PHASE_NL_DISPLAY`** (or TOML **`staged_plan_two_phase_nl_display`**) suppresses user-visible streaming of finalized plan JSON and adds a natural-language-only follow-up round (default off). Full table: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md#staged-planning-staged_plan_execution).

```bash
# Optional: export AGENT_API_BASE=… AGENT_MODEL=… API_KEY=… (or Web Settings / REPL /api-key set)
cargo build
./target/debug/crabmate repl    # or crabmate repl when on PATH
cd frontend-leptos && trunk build && cd ..
./target/debug/crabmate serve   # default :8080; release WASM: trunk build --release
```

### CLI

- **`crabmate repl`**: interactive chat; **`/`** commands and optional **`bash#:`**: [docs/en/CLI.md](docs/en/CLI.md). Under bearer without a key, use **`/api-key`**.
- **`crabmate chat`**: one-shot non-interactive; **`serve`**: HTTP + Web UI (shared logic with Web).
- **Common**: **`doctor`**, **`config`**, **`probe`** / **`models`**, **`bench`**, **`save-session`** / **`export-session`**, **`tool-replay`**, **`mcp list`**. Globals: **`--config`**, **`--workspace`**, **`--agent-role`**, **`--no-tools`**, **`--no-stream`**, etc.
- Config keys: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md); full subcommand list, benchmark, **`man crabmate`**: [docs/en/CLI.md](docs/en/CLI.md).

**Frontend**: `cd frontend-leptos && trunk build` (dev; **`--release`** for production), then **`crabmate serve`**. UI language in Settings; see `frontend-leptos/README.md`, [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md).

**Config**: default `config/*.toml` (embedded) + optional root **`config.toml`**; **`system_prompt_file`** → `config/prompts/default_system_prompt.md` (edit without rebuild). By default a thinking-discipline appendix is appended to the first `system` (editable **`config/prompts/thinking_avoid_echo_appendix.md`**, see [CONFIGURATION](docs/en/CONFIGURATION.md)). **Release / deb / man**: **[Build and packaging](#build-and-packaging)**.

**Switching models / gateway** (DeepSeek, MiniMax, Ollama, …): see **[Backend models](#backend-models)** above.

## Build and packaging

- **Toolchain**: **Rust 1.85+**, **Trunk** + **`wasm32-unknown-unknown`**; Linux / long-term memory notes: [AGENTS.md](AGENTS.md).
- **Build**: `cargo build` → `target/debug/crabmate`; **`--release`** → `target/release/crabmate`. With Web: **`cd frontend-leptos && trunk build`** first (**`--release`** for production WASM).
- **Checks**: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`; or [.pre-commit-config.yaml](.pre-commit-config.yaml).
- **E2E** (optional): after `frontend-leptos` build, **`cd e2e && npm ci && npx playwright install chromium && npm test`**. See [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md).
- **Install**: `cargo install --path .` (**does not** install man; use `.deb` or [man/crabmate.1](man/crabmate.1)). Regenerate man: `cargo run --bin crabmate-gen-man`.
- **One-shot packaging**: **`./scripts/package-release.sh`** → **`dist/`** with **`crabmate_<version>_<os>_<arch>.tar.gz`** (binary, `config/`, `frontend-leptos/dist`, man); on Linux with **`cargo-deb`** installed, also copies **`target/debian/crabmate_*.deb`** into **`dist/`**.
- **`.deb`**: [cargo-deb](https://github.com/kornelski/cargo-deb), or manually: frontend release + **`cargo deb`**, default output **`target/debian/`**. Details: [docs/en/CLI.md](docs/en/CLI.md) § Debian `.deb` packaging.

## Deployment and security

- **Listen address**: default **`127.0.0.1`**; **`0.0.0.0`** requires **`web_api_bearer_token`** or an explicit insecure flag ([docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md)).
- **Web API secret**: same as **`web_api_bearer_token`**; send **`Authorization: Bearer …`** or **`X-API-Key: …`** (either). The frontend may read **`localStorage["crabmate-api-bearer-token"]`** and sends both headers for compatibility with scripts/gateways.
- **Web Settings**: per-request **`client_llm`** (`api_base` / `model` / key) does not change server TOML; see [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md) § Web chat queue.
- **Workspace**: must stay under allowed roots; on Unix, **`openat2`** etc. reduce path risk—**not** a full sandbox. See [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md), [`src/path_workspace.rs`](src/path_workspace.rs).
- **Other**: **`web_search_api_key`** separate from main **`API_KEY`**; optional **SyncDefault Docker sandbox**: [docs/en/CONFIGURATION.md](docs/en/CONFIGURATION.md). Maintainers: [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md), [.cursor/rules/security-sensitive-surface.mdc](.cursor/rules/security-sensitive-surface.mdc).

## Project structure

Module map, call flow, **`GET /status`** observability, and **`src/`** index: [docs/en/DEVELOPMENT.md](docs/en/DEVELOPMENT.md).

## References

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek platform](https://platform.deepseek.com/)
- **MiniMax**: [Platform / docs](https://platform.minimaxi.com)
- **Zhipu GLM**: [Open platform](https://open.bigmodel.cn/) · [GLM-5 guide](https://docs.bigmodel.cn/cn/guide/models/text/glm-5)
- **Moonshot Kimi**: [Kimi API / Chat](https://platform.moonshot.cn/docs/api/chat)
