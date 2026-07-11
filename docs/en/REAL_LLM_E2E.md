# Real LLM E2E (manual opt-in, Victauri)

Runs only when **`REAL_LLM_E2E=1`**. Default **`cargo test`** / CI use SSE stubs and skip Victauri unless **`VICTAURI_E2E=1`**.

Canonical guide (Chinese): [`../真实LLM-E2E.md`](../真实LLM-E2E.md).

## Specs

| File | Purpose |
|------|---------|
| `desktop-tauri/src-tauri/tests/victauri_real_llm.rs` | Real vendor streaming (e.g. skills smoke, compile turn) |

## Quick start

```bash
unset NO_COLOR && cd frontend && trunk build

# Terminal 1: Tauri app
cd desktop-tauri/src-tauri
CM_E2E_FIXTURES=1 CM_DESKTOP_BACKEND_BIN=/path/to/target/debug/crabmate cargo tauri dev

# Terminal 2: real LLM tests
cd desktop-tauri/src-tauri
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 REAL_LLM_E2E=1 API_KEY=YOUR_API_KEY \
  cargo test --test victauri_real_llm -- --nocapture
```

Or: `./scripts/victauri-e2e.sh real_llm` (with **`REAL_LLM_E2E=1`** and **`API_KEY`** set).

## Environment

**`REAL_LLM_E2E=1`**, **`VICTAURI_E2E=1`**, **`CM_E2E_FIXTURES=1`**, **`API_KEY`**, optional **`CM_DESKTOP_BACKEND_BIN`**.

Not run in default CI. Do not commit API keys or raw artifacts with secrets.
