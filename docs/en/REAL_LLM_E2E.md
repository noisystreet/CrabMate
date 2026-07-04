# Real DeepSeek E2E (manual opt-in)

Playwright specs run only when **`REAL_LLM_E2E=1`**. Default CI / `npm test` uses SSE stubs and **does not** call a live model.

Full step-by-step (Chinese, canonical): [`../真实LLM-E2E.md`](../真实LLM-E2E.md).

## Specs

| File | Purpose |
|------|---------|
| `e2e/tests/real-llm-smoke.spec.ts` | Single turn “你有哪些技能”; streaming + busy state |
| `e2e/tests/real-llm-turn-layout.spec.ts` | Two turns + MD export; batch/final layout (not mega bubble) |

## Quick start (Turn layout, Tauri-aligned)

```bash
unset NO_COLOR && cd frontend && trunk build

# Terminal 1
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
cargo run -- --workspace /home/gzz/test serve --port 18888 --host 127.0.0.1

# Terminal 2
cd e2e
export REAL_LLM_E2E=1 E2E_PORT=18888 CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
npx playwright test tests/real-llm-turn-layout.spec.ts --workers=1 --retries=0
```

Use the same **`CM_CRABMATE_USER_DATA_DIR`** as the desktop app (`llm_overrides.json` + `secrets/client_llm`). Optional workspace override: **`REAL_LLM_WORKSPACE`**.

Smoke-only with Playwright `webServer`:

```bash
REAL_LLM_E2E=1 API_KEY=YOUR_API_KEY npx playwright test tests/real-llm-smoke.spec.ts --retries=0
```

Not run in CI. Do not commit real API keys.
