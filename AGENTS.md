# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust-based AI Agent powered by the DeepSeek API. It provides Web UI (Axum + React), TUI, and CLI interfaces. See `README.md` for full feature list and `docs/DEVELOPMENT.md` for architecture details.

### Required environment variable

- `API_KEY` — DeepSeek API key. Required at runtime; without it, the server starts but chat requests fail with `INTERNAL_ERROR`. Use `--dry-run` to verify config without making API calls.

### Running services

- **Backend + Web UI**: `API_KEY="..." cargo run -- --serve` (default port 8080)
- **Frontend dev server** (optional, for hot-reload): `cd frontend && npm run dev` (Vite proxies API calls to `:8080`)
- Frontend must be built (`cd frontend && npm run build`) before running the backend in serve mode, since it serves `frontend/dist` as static assets.

### Lint / Test / Build

Standard commands from `README.md`:

| Task | Command |
|------|---------|
| Rust build | `cargo build` |
| Rust tests | `cargo test` |
| Rust clippy | `cargo clippy` |
| Rust format check | `cargo fmt --check` |
| Frontend install | `cd frontend && npm install` |
| Frontend build | `cd frontend && npm run build` |

### Gotchas

- The project uses Rust **edition 2024**, which requires **Rust 1.85+**. The VM snapshot installs the latest stable toolchain. If `cargo build` fails with an edition error, run `rustup update stable && rustup default stable`.
- System libraries `libssl-dev` and `libssh2-1-dev` are required for the Rust build (installed by the VM snapshot).
- The `bc` command-line calculator is used by the `calc` tool at runtime. It may not be pre-installed; this causes `/health` to report `dep_bc` as degraded, but does not block the server from starting.
- `cargo fmt --check` may report pre-existing formatting differences in the codebase; this is not caused by agent changes.
