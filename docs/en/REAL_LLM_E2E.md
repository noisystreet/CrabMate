# Real LLM E2E (manual opt-in, Victauri)

Runs only when **`REAL_LLM_E2E=1`**. Default **`cargo test`** / CI use SSE stubs and skip Victauri unless **`VICTAURI_E2E=1`**.

Canonical guide (Chinese): [`../真实LLM-E2E.md`](../真实LLM-E2E.md).

Victauri lives **only** in the Client repo (`../crabmate-client`).

## Specs

| File | Purpose |
|------|---------|
| `../crabmate-client/desktop-tauri/src-tauri/tests/victauri_real_llm.rs` | Real vendor streaming (e.g. skills smoke, compile turn) |

## Quick start

```bash
unset NO_COLOR && cd ../crabmate-client && make frontend

# Terminal 1: backend (this repo)
cargo run -- serve --host 127.0.0.1 --port 18080

# Terminal 2: Tauri app (Client repo; skip connect page)
cd ../crabmate-client/desktop-tauri/src-tauri
CM_E2E_FIXTURES=1 CM_DESKTOP_SERVE_URL=http://127.0.0.1:18080/ cargo tauri dev

# Terminal 3: real LLM tests (same src-tauri)
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 REAL_LLM_E2E=1 API_KEY=YOUR_API_KEY \
  cargo test --test victauri_real_llm -- --nocapture
```

Or from the Client repo root: `./scripts/victauri-e2e.sh real_llm` (with **`REAL_LLM_E2E=1`** and **`API_KEY`** set; script starts **`serve`**).

## Environment

**`REAL_LLM_E2E=1`**, **`VICTAURI_E2E=1`**, **`CM_E2E_FIXTURES=1`**, **`API_KEY`**, **`CM_DESKTOP_SERVE_URL`** (when not using the one-shot script).

Not run in default CI. Do not commit API keys or raw artifacts with secrets.
