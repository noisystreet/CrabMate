#!/usr/bin/env bash
# 本地 E2E：构建 frontend/dist 后运行 Playwright（与 CI e2e job 对齐，不调用真实 LLM）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

unset NO_COLOR
unset CARGO_TERM_COLOR

if [[ ! -f frontend/dist/index.html ]]; then
  echo "==> trunk build (frontend/dist required)"
  (cd frontend && trunk build)
fi

if [[ -x target/release/crabmate ]]; then
  export CI=true
fi

echo "==> playwright test"
(cd e2e && npm ci && npx playwright install chromium)
(cd e2e && npm test)
