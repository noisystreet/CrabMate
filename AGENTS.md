# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust-based AI Agent that calls **OpenAI-compatible** `chat/completions` backends (e.g. **DeepSeek**, **MiniMax**, **Zhipu GLM**, **Moonshot Kimi**, **Ollama**). It provides Web UI (Axum + React) and CLI (interactive terminal / `chat` single-shot / `serve`). See `README.md` for quick start and feature overview; `docs/CONFIGURATION.md` for env vars and TOML; `docs/CLI.md` for subcommands and routes; `docs/TOOLS.md` for built-in tools; `docs/DEVELOPMENT.md` for architecture (module index). If you change module layout or layering, update `docs/DEVELOPMENT.md` per `.cursor/rules/architecture-docs-sync.mdc`.

### Environment variable `API_KEY`

- **Default (`llm_http_auth_mode = bearer` in config)**：`API_KEY` is the cloud vendor Bearer token. Required for `serve` / `repl` / `chat` / `bench` / `models` / `probe`. **`config`（`cargo run -- config` / `--dry-run`）、`doctor` 与 `save-session`（别名 `export-session`）不要求 `API_KEY`**。
- **Local OpenAI-compatible backends（e.g. Ollama）**：set **`llm_http_auth_mode = "none"`**（or **`AGENT_LLM_HTTP_AUTH_MODE=none`**）so CrabMate does **not** send `Authorization` to `chat/completions` / `models`; then `serve` / `repl` / `chat` / `bench` / `models` / `probe` can run **without** `API_KEY`. With `bearer` and a wrong key the server may start but chat fails (`INTERNAL_ERROR`).
- **MiniMax（OpenAI-compatible）**：point **`api_base`** at the vendor root (e.g. **`https://api.minimaxi.com/v1`** per [OpenAI API 兼容](https://platform.minimaxi.com/docs/api-reference/text-openai-api)) and keep **`llm_http_auth_mode = bearer`** with **`API_KEY`** set. **`model`** values exercised in this repo include **`MiniMax-M2.7`**, **`MiniMax-M2.7-highspeed`**, and **`MiniMax-M2.5`** (see vendor console for the full list). Despite doc examples with **`system`**, the live API often returns **`invalid message role: system`**; set **`llm_fold_system_into_user = true`** to merge system into **`user`** (embedded **`config/default_config.toml`** defaults to **`false`** for DeepSeek-first setups; MiniMax usually needs **`true`**). Keep **`false`** only if your gateway accepts a standalone **`system`** message. Optional **`llm_reasoning_split = true`** / **`AGENT_LLM_REASONING_SPLIT`** adds **`reasoning_split: true`**; streaming **`delta.reasoning_details`** is folded into CrabMate’s **`reasoning_content`** path. See **`docs/CONFIGURATION.md`** (“MiniMax”).
- **Zhipu GLM（OpenAI-compatible）**：**`api_base`** e.g. **`https://open.bigmodel.cn/api/paas/v4`**, **`model`** e.g. **`glm-5`**, **`API_KEY`** as Bearer — same as the vendor minimal cURL (**`model` + `messages` + `stream: true`**, no **`thinking`**). CrabMate also sends standard **`max_tokens`** / **`temperature`** from config. Optional **`llm_bigmodel_thinking = true`** / **`AGENT_LLM_BIGMODEL_THINKING`** adds **`thinking: { "type": "enabled" }`** per [GLM-5 docs](https://docs.bigmodel.cn/cn/guide/models/text/glm-5). See **`docs/CONFIGURATION.md`** (“智谱 GLM”).
- **Moonshot Kimi（OpenAI-compatible）**：**`api_base`** **`https://api.moonshot.cn/v1`**, **`model`** e.g. **`kimi-k2.5`** / **`kimi-k2-0905-preview`** (see [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat)), **`API_KEY`** as Bearer. Outbound **`temperature`** is coerced per model ID (**`kimi-k2.5*`** and **`kimi-k2-thinking*`** → **1.0**; other **`kimi-k2-*`** → **0.6**) to match vendor constraints. Optional **`llm_kimi_thinking_disabled`** / **`AGENT_LLM_KIMI_THINKING_DISABLED`** sends **`thinking: { "type": "disabled" }`** for **kimi-k2.5** only (vendor default is effectively enabled). See **`docs/CONFIGURATION.md`** (“Moonshot（Kimi）”).

### Running services

- **Backend + Web UI**: `API_KEY="..." cargo run -- serve` (subcommand `serve`; default port 8080, binds **127.0.0.1** only). For LAN access use `serve --host 0.0.0.0` (see README). Legacy `cargo run -- --serve` still works. Optional global `--log /path/to.log` appends logs and mirrors to stderr. Without `RUST_LOG`, `serve` defaults to **info**; `repl` / `chat` / `bench` / `config` / `save-session` (alias `export-session`) default to **warn** unless you set `RUST_LOG` or `--log`. **`POST /config/reload`** (same auth as protected APIs) hot-reloads most `AgentConfig` fields without restarting `serve`; see **`docs/CONFIGURATION.md`**.
- **CLI diagnostics**: `cargo run -- doctor` — human-readable check (Rust/npm/frontend paths, allowlist size, redacted secrets); **no `API_KEY`**. **`save-session`** exports chat JSON/Markdown to `<workspace>/.crabmate/exports/` (same shape as Web; alias `export-session`); **no `API_KEY`**. **`mcp list`** prints the in-process MCP stdio session cache (merged OpenAI tool names) when MCP is enabled; **`mcp list --probe`** tries one connection (starts `mcp_command`); **no `API_KEY`**. `cargo run -- models` / `probe` use `GET {api_base}/models` with Bearer only when `llm_http_auth_mode=bearer`; with `none`, no `Authorization` header is sent.
- **Frontend dev server** (optional, for hot-reload): `cd frontend && npm run dev` (Vite proxies API calls to `:8080`)
- Before running the backend in `serve` mode, build a static UI: either `cd frontend && npm run build` (outputs `frontend/dist`) or `cd frontend-leptos && trunk build` (outputs `frontend-leptos/dist`). If `frontend-leptos/dist` exists, the server prefers it over `frontend/dist`.

### Lint / Test / Build

Standard commands from `README.md`:

| Task | Command |
|------|---------|
| Rust build | `cargo build` |
| Rust tests | `cargo test` |
| Rust tests (nightly) | `cargo +nightly test` |
| Rust clippy | `cargo clippy` |
| 依赖漏洞（RustSec，需安装 `cargo-audit`） | `cargo audit` |
| 依赖许可证/来源（需安装 `cargo-deny`） | `cargo deny check licenses bans sources`（配置见根目录 `deny.toml`；CI 见 `.github/workflows/dependency-security.yml`） |
| Rust format check | `cargo fmt --check` |
| TypeScript check | `cd frontend && npx tsc -b --noEmit` |
| Frontend install | `cd frontend && npm install` |
| Frontend build | `cd frontend && npm run build` |
| Regenerate `man` page (troff) | `cargo run --bin crabmate-gen-man`（写入 `man/crabmate.1`） |

### Gotchas

- **排障摘要**：模型可调用工具 **`diagnostic_summary`**（参数均可选）收集只读、脱敏信息：Rust 工具链版本、`target/` 与常见路径是否存在、关键环境变量是否已设置（**不输出任何变量值**；与 `API_KEY` 同类变量亦不报告长度）。勿将真实密钥粘贴进对话或工具入参。
- The project uses Rust **edition 2024**, which requires **Rust 1.85+**. The VM snapshot installs the latest stable toolchain. If `cargo build` fails with an edition error, run `rustup update stable && rustup default stable`.
- **Rust nightly** is pre-installed in the environment. You can use `cargo +nightly test` and similar commands directly.
- System libraries `libssl-dev` and `libssh2-1-dev` are required for the Rust build (installed by the VM snapshot).
- **长期记忆（fastembed）**：依赖 ONNX Runtime 与 **libstdc++**；仓库根 **`.cargo/config.toml`** 在 Linux x86_64 上将链接器设为 **`gcc`**，以便正确解析 `-lstdc++`。若你在极简环境中链接失败，请安装 **`g++`**（或等价的 `libstdc++` 开发包）后再 `cargo build` / `cargo test`。
- Web **optional** SQLite session store uses **`rusqlite` with the `bundled` feature** (ships SQLite via `libsqlite3-sys`); no system `libsqlite3` install is required for that path.
- The `bc` command-line calculator is used by the `calc` tool at runtime. It may not be pre-installed; this causes `/health` to report `dep_bc` as degraded, but does not block the server from starting.
- `clang-format` is used by `format_file` / `format_check_file` for C/C++ sources. If missing, `/health` may report `dep_clang_format` as degraded; C/C++ formatting tools will return an explanatory error.
- `cmake` and `ctest` are on the `run_command` allowlist for configuring/building/testing CMake projects. If missing, `/health` may report `dep_cmake` / `dep_ctest` as degraded. `mkdir` is also allowlisted for script-style directory creation (same argument rules as other `run_command` invocations). Note: `run_command` rejects any arg containing `..` or starting with `/`, so prefer relative `-S`/`-B` and `--build` paths (avoid absolute `-D` values in args).
- `c++filt` (Itanium demangler) is on the default `run_command` allowlist for demangling linker/stack symbols. If missing, `/health` may report `dep_cxxfilt` as degraded.
- GNU **Binutils**-style tools on the default `run_command` allowlist for read-only binary inspection: `objdump`, `nm`, `readelf`, `strings`, `size`, and `ar`. If missing, `/health` may report `dep_objdump` / `dep_nm` / `dep_readelf` / `dep_strings_binutils` / `dep_size` / `dep_ar` as degraded. Same `run_command` rules: no `..` or `/`-prefixed args.
- **`npm`** and **`python3`** are on the default `run_command` allowlist (e.g. for `error_output_playbook` suggestions). If missing, `/health` may report `dep_npm` / `dep_python3` as degraded; related commands will fail until installed.
- `autoreconf`, `autoconf`, `automake`, and `aclocal` are on the default `run_command` allowlist for Autotools maintenance; they execute project `configure.ac` / `Makefile.am` logic—only use in trusted workspaces. For a narrower allowlist (e.g. in production), override **`allowed_commands`** in your config or set **`AGENT_ALLOWED_COMMANDS`**; defaults ship in **`config/tools.toml`**.
- Default **`allowed_commands`** includes common Linux utilities (e.g. `stat`, `grep`, `diff`, `jq`, `ps`, `zcat`), **`python3` / `npm`**, and **`git` / `cargo` / `rustc`**; they can modify repos or run build scripts—treat workspaces as trusted.
- Source analysis tools (`shellcheck_check`, `cppcheck_analyze`, `semgrep_scan`, `hadolint_check`, `bandit_scan`, `lizard_complexity`) require corresponding CLIs installed locally. If missing, `/health` reports `dep_shellcheck` / `dep_cppcheck` / `dep_semgrep` / `dep_hadolint` / `dep_bandit` / `dep_lizard` as degraded. These are read-only analysis tools and do not modify files.
- **Lint**：仓库 **pre-commit** 使用 **`cargo clippy --all-targets --all-features -- -D warnings`**（见 **`.pre-commit-config.yaml`** 与 **`.cursor/rules/pre-commit-before-commit.mdc`**）。**提交前**须通过；仅本地快速试探时可运行不带 `-D warnings` 的 **`cargo clippy`**，但不应在 hook 未通过时代为提交。
- **`cargo fmt --check`**：若与 **`cargo fmt`** 结果不一致，先执行 **`cargo fmt --all`** 再提交；pre-commit 也会格式化 Rust 代码。
- The `rfd` crate (file dialog) is a dependency but won't work headlessly; this doesn't affect the web server mode.
- **pre-commit install** may fail with `core.hooksPath` set. Run `git config --unset-all core.hooksPath` first, then `pre-commit install && pre-commit install --hook-type commit-msg`.
- When starting the server with `--host 0.0.0.0` (non-loopback), you must either set `AGENT_WEB_API_BEARER_TOKEN` or `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`; otherwise the server refuses to start.
