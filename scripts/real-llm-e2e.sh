#!/usr/bin/env bash
# 真实 LLM Playwright E2E（opt-in）。见 docs/真实LLM-E2E.md
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E_DIR="${REPO_ROOT}/e2e"

export REAL_LLM_E2E=1
export E2E_PORT="${E2E_PORT:-18888}"
export CM_CRABMATE_USER_DATA_DIR="${CM_CRABMATE_USER_DATA_DIR:-${HOME}/.local/share/crabmate}"

PW_ARGS=(--workers=1 --retries=0)
if [[ -n "${REAL_LLM_GREP:-}" ]]; then
  PW_ARGS+=(-g "${REAL_LLM_GREP}")
fi

usage() {
  cat <<EOF
Usage: $(basename "$0") [smoke|analyze|compile|layout|all]

  smoke   — real-llm-smoke.spec.ts
  analyze — single turn 分析当前目录
  compile — single turn 编译 hpcg（Turn 布局）
  layout  — two turns + export（完整）
  all     — 以上全部（默认）

Environment:
  E2E_PORT, CM_CRABMATE_USER_DATA_DIR, REAL_LLM_WORKSPACE,
  REAL_LLM_CAPTURE=1  — 通过时也写入 e2e/artifacts/real-llm/
  REAL_LLM_STRICT_STREAM_LAYOUT=1 — 流式 DOM violations 非空则失败
  REAL_LLM_STREAM_MONITOR=0   — 关闭 compile 轮 DOM 采样
  REAL_LLM_GREP       — 传给 playwright -g

Prerequisite: frontend/dist (cd frontend && trunk build) and serve on E2E_PORT
EOF
}

TARGET="${1:-all}"
cd "${E2E_DIR}"

case "${TARGET}" in
  smoke)
    npx playwright test tests/real-llm-smoke.spec.ts "${PW_ARGS[@]}"
    ;;
  analyze)
    npx playwright test tests/real-llm-turn-layout-analyze.spec.ts "${PW_ARGS[@]}"
    ;;
  compile)
    npx playwright test tests/real-llm-turn-layout-compile.spec.ts "${PW_ARGS[@]}"
    ;;
  layout)
    npx playwright test tests/real-llm-turn-layout.spec.ts "${PW_ARGS[@]}"
    ;;
  all)
    npx playwright test \
      tests/real-llm-smoke.spec.ts \
      tests/real-llm-turn-layout-analyze.spec.ts \
      tests/real-llm-turn-layout-compile.spec.ts \
      tests/real-llm-turn-layout.spec.ts \
      "${PW_ARGS[@]}"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    echo "Unknown target: ${TARGET}" >&2
    usage >&2
    exit 1
    ;;
esac
