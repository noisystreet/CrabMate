# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust-based AI Agent powered by the DeepSeek API. It provides Web UI (Axum + React), TUI, and CLI interfaces. See `README.md` for full feature list and `docs/DEVELOPMENT.md` for architecture details.

### Required environment variable

- `API_KEY` — DeepSeek API key. Required at runtime; without it, the server starts but chat requests fail with `INTERNAL_ERROR`. Use `--dry-run` to verify config without making API calls.

### Running services

- **Backend + Web UI**: `API_KEY="..." cargo run -- --serve` (default port 8080, binds **127.0.0.1** only). For LAN access use `--host 0.0.0.0` (see README security notes).
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

- The project uses Rust **edition 2024**, which requires **Rust 1.85+**. The VM snapshot installs the latest stable toolchain. If `cargo build` fails with an edition error, run `rustup update stable && rustup default stable`.
- **Rust nightly** is pre-installed in the environment. You can use `cargo +nightly test` and similar commands directly.
- System libraries `libssl-dev` and `libssh2-1-dev` are required for the Rust build (installed by the VM snapshot).
- The `bc` command-line calculator is used by the `calc` tool at runtime. It may not be pre-installed; this causes `/health` to report `dep_bc` as degraded, but does not block the server from starting.
- `cargo clippy -- -D warnings` will fail on existing code (pre-existing warnings like `too_many_arguments`, `collapsible_if`). Use `cargo clippy` without `-D warnings` for a non-blocking lint check.
- `cargo fmt --check` may report pre-existing formatting differences in the codebase; this is not caused by agent changes.
- The `rfd` crate (file dialog) is a dependency but won't work headlessly; this doesn't affect the web server mode.
- The vendored `tui-markdown` crate is at `vendor/tui-markdown/` and is referenced via `path` in `Cargo.toml`.
