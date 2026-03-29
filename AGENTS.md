# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust-based AI Agent powered by the DeepSeek API. It provides Web UI (Axum + React) and CLI (REPL / single-shot / `--serve`). See `README.md` for quick start and feature overview; `docs/CONFIGURATION.md` for env vars and TOML; `docs/CLI.md` for subcommands and routes; `docs/TOOLS.md` for built-in tools; `docs/DEVELOPMENT.md` for architecture (module index). If you change module layout or layering, update `docs/DEVELOPMENT.md` per `.cursor/rules/architecture-docs-sync.mdc`.

### Environment variable `API_KEY`

- **Default (`llm_http_auth_mode = bearer` in config)**：`API_KEY` is the cloud vendor Bearer token. Required for `serve` / `repl` / `chat` / `bench` / `models` / `probe`. **`config`（`cargo run -- config` / `--dry-run`）、`doctor` 与 `save-session`（别名 `export-session`）不要求 `API_KEY`**。
- **Local OpenAI-compatible backends（e.g. Ollama）**：set **`llm_http_auth_mode = "none"`**（or **`AGENT_LLM_HTTP_AUTH_MODE=none`**）so CrabMate does **not** send `Authorization` to `chat/completions` / `models`; then `serve` / `repl` / `chat` / `bench` / `models` / `probe` can run **without** `API_KEY`. With `bearer` and a wrong key the server may start but chat fails (`INTERNAL_ERROR`).

### Running services

- **Backend + Web UI**: `API_KEY="..." cargo run -- serve` (subcommand `serve`; default port 8080, binds **127.0.0.1** only). For LAN access use `serve --host 0.0.0.0` (see README). Legacy `cargo run -- --serve` still works. Optional global `--log /path/to.log` appends logs and mirrors to stderr. Without `RUST_LOG`, `serve` defaults to **info**; `repl` / `chat` / `bench` / `config` / `save-session` (alias `export-session`) default to **warn** unless you set `RUST_LOG` or `--log`.
- **CLI diagnostics**: `cargo run -- doctor` — human-readable check (Rust/npm/frontend paths, allowlist size, redacted secrets); **no `API_KEY`**. **`save-session`** exports chat JSON/Markdown to `<workspace>/.crabmate/exports/` (same shape as Web; alias `export-session`); **no `API_KEY`**. **`mcp list`** prints the in-process MCP stdio session cache (merged OpenAI tool names) when MCP is enabled; **`mcp list --probe`** tries one connection (starts `mcp_command`); **no `API_KEY`**. `cargo run -- models` / `probe` use `GET {api_base}/models` with Bearer only when `llm_http_auth_mode=bearer`; with `none`, no `Authorization` header is sent.
- **Frontend dev server** (optional, for hot-reload): `cd frontend && npm run dev` (Vite proxies API calls to `:8080`)
- Frontend must be built (`cd frontend && npm run build`) before running the backend in serve mode, since it serves `frontend/dist` as static assets.

### Lint / Test / Build

Standard commands from `README.md`:

| Task | Command |
|------|---------|
| Rust build | `cargo build` |
| Rust tests | `cargo test` |
| Rust tests (nightly) | `cargo +nightly test` |
| Rust clippy | `cargo clippy` |
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
- `cmake` is on the `run_command` allowlist for configuring/building CMake projects. If missing, `/health` may report `dep_cmake` as degraded. Note: `run_command` rejects any arg containing `..` or starting with `/`, so prefer relative `-S`/`-B` and `--build` paths (avoid absolute `-D` values in args).
- `c++filt` (Itanium demangler) is on the dev `run_command` allowlist for demangling linker/stack symbols. If missing, `/health` may report `dep_cxxfilt` as degraded.
- GNU **Binutils**-style tools on the default `run_command` allowlist for read-only binary inspection: `objdump`, `nm`, `readelf`, `strings`, `size` (and `ar` in dev). If missing, `/health` may report `dep_objdump` / `dep_nm` / `dep_readelf` / `dep_strings_binutils` / `dep_size` / `dep_ar` as degraded. Same `run_command` rules: no `..` or `/`-prefixed args.
- **`npm`** and **`python3`** are on the default dev `run_command` allowlist (e.g. for `error_output_playbook` suggestions). If missing, `/health` may report `dep_npm` / `dep_python3` as degraded; related commands will fail until installed.
- `autoreconf`, `autoconf`, `automake`, and `aclocal` are on the default dev-oriented `run_command` allowlist for Autotools maintenance; they execute project `configure.ac` / `Makefile.am` logic—only use in trusted workspaces. Not in `allowed_commands_prod` by default.
- Default **dev** `allowed_commands` also includes common Linux utilities (e.g. `stat`, `grep`, `diff`, `jq`, `ps`, `zcat`), **`python3` / `npm`** (for diagnostics-style suggestions such as `error_output_playbook`), and **`git` / `cargo` / `rustc`** for typical development workflows; they can modify repos or run build scripts—treat workspaces as trusted. **`prod`** uses a narrower `allowed_commands_prod` without compilers, `git`, or `cargo` by default.
- Source analysis tools (`shellcheck_check`, `cppcheck_analyze`, `semgrep_scan`, `hadolint_check`, `bandit_scan`, `lizard_complexity`) require corresponding CLIs installed locally. If missing, `/health` reports `dep_shellcheck` / `dep_cppcheck` / `dep_semgrep` / `dep_hadolint` / `dep_bandit` / `dep_lizard` as degraded. These are read-only analysis tools and do not modify files.
- **Lint**：仓库 **pre-commit** 使用 **`cargo clippy --all-targets --all-features -- -D warnings`**（见 **`.pre-commit-config.yaml`** 与 **`.cursor/rules/pre-commit-before-commit.mdc`**）。**提交前**须通过；仅本地快速试探时可运行不带 `-D warnings` 的 **`cargo clippy`**，但不应在 hook 未通过时代为提交。
- **`cargo fmt --check`**：若与 **`cargo fmt`** 结果不一致，先执行 **`cargo fmt --all`** 再提交；pre-commit 也会格式化 Rust 代码。
- The `rfd` crate (file dialog) is a dependency but won't work headlessly; this doesn't affect the web server mode.
- **pre-commit install** may fail with `core.hooksPath` set. Run `git config --unset-all core.hooksPath` first, then `pre-commit install && pre-commit install --hook-type commit-msg`.
- When starting the server with `--host 0.0.0.0` (non-loopback), you must either set `AGENT_WEB_API_BEARER_TOKEN` or `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`; otherwise the server refuses to start.
