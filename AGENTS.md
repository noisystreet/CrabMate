# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust-based AI Agent powered by the DeepSeek API. It provides Web UI (Axum + React) and CLI (REPL / single-shot / `--serve`). See `README.md` for full feature list and `docs/DEVELOPMENT.md` for architecture details (including module index). If you change module layout or layering, update `docs/DEVELOPMENT.md` per `.cursor/rules/architecture-docs-sync.mdc`.

### Required environment variable

- `API_KEY` — DeepSeek API key. Required at runtime; without it, the server **refuses to start** (exits with "未设置环境变量 API_KEY"). With an invalid key the server starts normally but chat requests fail with `INTERNAL_ERROR`. Use `--dry-run` to verify config without making API calls.

### Running services

- **Backend + Web UI**: `API_KEY="..." cargo run -- --serve` (default port 8080, binds **127.0.0.1** only). For LAN access use `--host 0.0.0.0` (see README security notes). Optional `--log /path/to.log` appends `log`/`env_logger` output to a file (with `RUST_LOG`) and mirrors to stderr. Without `RUST_LOG`, `--serve` defaults to **info** logs; other CLI modes default to **warn** (no `info`) unless you set `RUST_LOG` or `--log`.
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

### Gotchas

- **排障摘要**：模型可调用工具 **`diagnostic_summary`**（参数均可选）收集只读、脱敏信息：Rust 工具链版本、`target/` 与常见路径是否存在、关键环境变量是否已设置（**不输出任何变量值**；与 `API_KEY` 同类变量亦不报告长度）。勿将真实密钥粘贴进对话或工具入参。
- The project uses Rust **edition 2024**, which requires **Rust 1.85+**. The VM snapshot installs the latest stable toolchain. If `cargo build` fails with an edition error, run `rustup update stable && rustup default stable`.
- **Rust nightly** is pre-installed in the environment. You can use `cargo +nightly test` and similar commands directly.
- System libraries `libssl-dev` and `libssh2-1-dev` are required for the Rust build (installed by the VM snapshot).
- The `bc` command-line calculator is used by the `calc` tool at runtime. It may not be pre-installed; this causes `/health` to report `dep_bc` as degraded, but does not block the server from starting.
- `clang-format` is used by `format_file` / `format_check_file` for C/C++ sources. If missing, `/health` may report `dep_clang_format` as degraded; C/C++ formatting tools will return an explanatory error.
- `cmake` is on the `run_command` allowlist for configuring/building CMake projects. If missing, `/health` may report `dep_cmake` as degraded. Note: `run_command` rejects any arg containing `..` or starting with `/`, so prefer relative `-S`/`-B` and `--build` paths (avoid absolute `-D` values in args).
- `c++filt` (Itanium demangler) is on the dev `run_command` allowlist for demangling linker/stack symbols. If missing, `/health` may report `dep_cxxfilt` as degraded.
- `autoreconf`, `autoconf`, `automake`, and `aclocal` are on the default dev-oriented `run_command` allowlist for Autotools maintenance; they execute project `configure.ac` / `Makefile.am` logic—only use in trusted workspaces. Not in `allowed_commands_prod` by default.
- **Lint**：仓库 **pre-commit** 使用 **`cargo clippy --all-targets --all-features -- -D warnings`**（见 **`.pre-commit-config.yaml`** 与 **`.cursor/rules/pre-commit-before-commit.mdc`**）。**提交前**须通过；仅本地快速试探时可运行不带 `-D warnings` 的 **`cargo clippy`**，但不应在 hook 未通过时代为提交。
- **`cargo fmt --check`**：若与 **`cargo fmt`** 结果不一致，先执行 **`cargo fmt --all`** 再提交；pre-commit 也会格式化 Rust 代码。
- The `rfd` crate (file dialog) is a dependency but won't work headlessly; this doesn't affect the web server mode.
- **pre-commit install** may fail with `core.hooksPath` set. Run `git config --unset-all core.hooksPath` first, then `pre-commit install && pre-commit install --hook-type commit-msg`.
- When starting the server with `--host 0.0.0.0` (non-loopback), you must either set `AGENT_WEB_API_BEARER_TOKEN` or `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`; otherwise the server refuses to start.
