# Real DeepSeek E2E (manual opt-in)

Playwright specs run only when **`REAL_LLM_E2E=1`**. Default CI uses SSE stubs.

Canonical guide (Chinese): [`../真实LLM-E2E.md`](../真实LLM-E2E.md).

## Specs

| File | Purpose |
|------|---------|
| `real-llm-smoke.spec.ts` | Single turn connectivity |
| `real-llm-turn-layout-analyze.spec.ts` | Single turn “分析当前目录” |
| `real-llm-turn-layout-compile.spec.ts` | Single turn “编译 hpcg” + layout export |
| `real-llm-turn-layout.spec.ts` | Two turns (full integration) |

Shared helpers: `e2e/tests/helpers/real-llm.ts`.

## Quick start

```bash
unset NO_COLOR && cd frontend && trunk build

export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
cargo run -- --workspace /home/gzz/test serve --port 18888 --host 127.0.0.1

export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
./scripts/real-llm-e2e.sh compile
```

From `e2e/`: `npm run test:real-llm:compile` (same script).

## Failure artifacts

On failure (or when **`REAL_LLM_CAPTURE=1`**): `e2e/artifacts/real-llm/<timestamp>_<test>/` with `meta.json`, `turn-layout-report.json`, `export.md`, `export.json`. Gitignored.

## Environment

`REAL_LLM_E2E=1`, `E2E_PORT`, `CM_CRABMATE_USER_DATA_DIR`, `REAL_LLM_WORKSPACE`, `REAL_LLM_CAPTURE`, `REAL_LLM_GREP`.

Not run in default CI. Do not commit API keys or raw artifacts with secrets.
