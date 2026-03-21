# AGENTS.md

## Cursor Cloud specific instructions

### Overview

CrabMate is a Rust AI Agent (DeepSeek API) with a Vite+React+TypeScript web frontend. The backend serves the frontend as static files from `frontend/dist/`.

### System requirements

- **Rust stable >= 1.85** (edition 2024 in `Cargo.toml`). Run `rustup update stable && rustup default stable` if the installed version is too old.
- **Rust nightly** — 预装在环境中，可直接使用 `cargo +nightly test` 等命令。
- **Node.js >= 22** (for the frontend build)
- **System packages**: `build-essential`, `pkg-config`, `libssl-dev`, `libssh2-1-dev`

### Key commands (see README.md for full details)

| Task | Command |
|------|---------|
| Install frontend deps | `cd frontend && npm install` |
| Build frontend | `cd frontend && npm run build` |
| Frontend dev server | `cd frontend && npm run dev` (port 5173, proxies to backend 8080) |
| Build backend | `cargo build` |
| Run tests | `cargo test` |
| Run tests (nightly) | `cargo +nightly test` |
| Lint (Rust) | `cargo clippy` |
| Format check (Rust) | `cargo fmt --check` |
| Run server | `API_KEY="..." cargo run -- --serve` (port 8080) |
| Dry-run config check | `API_KEY="..." cargo run -- --dry-run` |

### Non-obvious caveats

- The `API_KEY` environment variable (DeepSeek API key) is **required** to start the server. Without it, `cargo run` will fail at config validation. For testing server startup without a real key, use a placeholder: `API_KEY=test cargo run -- --dry-run`.
- The frontend must be built (`npm run build` in `frontend/`) **before** starting the backend in web mode (`--serve`), because the backend serves static files from `frontend/dist/`. If `frontend/dist/` is missing, the `--dry-run` check will report it and the web UI won't load.
- `cargo fmt --check` currently reports formatting differences in the codebase — this is a pre-existing condition, not a regression from your changes.
- The `ashpd` dependency triggers a future-incompatibility warning during build — safe to ignore.
- The `bc` command is used by the calculator tool. If missing, the health endpoint reports `dep_bc` as degraded but the server still runs fine.
