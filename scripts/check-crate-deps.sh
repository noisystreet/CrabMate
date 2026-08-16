#!/usr/bin/env bash
# 模块 DAG 禁边自检（S2 单包后：原 workspace crate 禁边改为 src/cm_* 引用）。
# 失败时打印违规边；退出码非 0。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

# 在 from_dir 的 .rs 中禁止出现 to_path（如 crate::cm_internal）
check_forbidden_mod() {
  local from_dir="$1"
  local needle="$2"
  local why="$3"
  if [[ ! -d "$from_dir" ]]; then
    echo "MISSING: ${from_dir} (${why})"
    fail=1
    return
  fi
  local hits
  hits="$(grep -R --include='*.rs' -n -F "$needle" "$from_dir" || true)"
  if [[ -n "$hits" ]]; then
    echo "FORBIDDEN: ${from_dir} must not reference ${needle} (${why})"
    printf '%s\n' "$hits" | head -40
    fail=1
  else
    echo "ok: ${from_dir} ↛ ${needle}"
  fi
}

echo "== module dependency policy =="
check_forbidden_mod src/cm_workflow crate::cm_internal \
  "workflow may only use cm_approval for Web approval SSE"
check_forbidden_mod src/cm_tools crate::cm_internal \
  "tools must not depend on internal facade"
check_forbidden_mod src/cm_agent crate::cm_internal \
  "agent module must not depend on internal facade"
check_forbidden_mod src/cm_approval crate::cm_internal \
  "approval types must stay below internal"
check_forbidden_mod src/cm_web_host crate::cm_internal \
  "web host must not depend on internal facade"

if [[ "$fail" -ne 0 ]]; then
  echo "dependency policy FAILED"
  exit 1
fi
echo "dependency policy OK"
