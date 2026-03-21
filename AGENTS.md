# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

CrabMate is a Rust AI Agent wrapping the DeepSeek LLM API with function-calling capabilities. It includes:
- **Rust backend** (Axum HTTP server) in `src/`
- **React/TypeScript frontend** (Vite + Tailwind + DaisyUI) in `frontend/`
- A TUI mode and CLI modes for terminal usage

### Prerequisites

- **Rust toolchain**: edition 2024 requires Rust 1.85+. The environment snapshot installs stable Rust via rustup.
- **Node.js 22+** and npm for the frontend.
- **System packages**: `libssl-dev` and `pkg-config` are required for the `openssl-sys` crate to compile. The environment snapshot installs these.
- **`API_KEY` env var**: Required at runtime. Set it to a valid [DeepSeek API key](https://platform.deepseek.com/). Without it, the server refuses to start.

### Common commands

See `README.md` for full details. Quick reference:

| Task | Command |
|------|---------|
| Install frontend deps | `cd frontend && npm install` |
| Build frontend | `cd frontend && npm run build` |
| Frontend dev server | `cd frontend && npm run dev` (proxies API to backend on :8080) |
| Build backend | `cargo build` |
| Run tests | `cargo test` |
| Clippy lint | `cargo clippy` |
| Format check | `cargo fmt --check` |
| TypeScript check | `cd frontend && npx tsc -b --noEmit` |
| Start web server | `API_KEY=<key> cargo run -- --serve 8080` |
| Dry-run config check | `API_KEY=<key> cargo run -- --dry-run` |

### Gotchas

- The frontend must be built (`npm run build` in `frontend/`) before starting the backend in `--serve` mode; the backend serves static files from `frontend/dist/`.
- `cargo clippy -- -D warnings` will fail on existing code (pre-existing warnings like `too_many_arguments`, `collapsible_if`). Use `cargo clippy` without `-D warnings` for a non-blocking lint check.
- `cargo fmt --check` also reports minor pre-existing formatting diffs (trailing whitespace). This is expected.
- The `rfd` crate (file dialog) is a dependency but won't work headlessly; this doesn't affect the web server mode.
- The vendored `tui-markdown` crate is at `vendor/tui-markdown/` and is referenced via `path` in `Cargo.toml`.
