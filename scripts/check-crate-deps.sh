#!/usr/bin/env bash
# 工作区 crate 依赖方向自检（降低耦合第一轮门禁）。
# 失败时打印违规边；退出码非 0。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

check_forbidden() {
  local pkg="$1"
  local forbidden="$2"
  local why="$3"
  local tree
  tree="$(cargo tree -p "$pkg" -e normal --prefix none 2>/dev/null || true)"
  if printf '%s\n' "$tree" | grep -qE "^${forbidden} "; then
    echo "FORBIDDEN: ${pkg} must not depend on ${forbidden} (${why})"
    cargo tree -p "$pkg" -e normal -i "$forbidden" --prefix indent 2>/dev/null | head -40 || true
    fail=1
  else
    echo "ok: ${pkg} ↛ ${forbidden}"
  fi
}

echo "== crate dependency policy =="
check_forbidden crabmate-workflow crabmate-internal \
  "workflow may only use crabmate-approval for Web approval SSE"
check_forbidden crabmate-tools crabmate-internal \
  "tools must not depend on internal facade"
check_forbidden crabmate-agent crabmate-internal \
  "agent crate must not depend on internal facade"
check_forbidden crabmate-approval crabmate-internal \
  "approval types must stay below internal"

if [[ "$fail" -ne 0 ]]; then
  echo "dependency policy FAILED"
  exit 1
fi
echo "dependency policy OK"
