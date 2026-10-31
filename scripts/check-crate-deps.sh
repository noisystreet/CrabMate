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
  # 目标是目录或单个文件（`src/cm_names.rs` 这类叶子模块单独成文件）。
  if [[ ! -e "$from_dir" ]]; then
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

# 工具名边界（#1155）：`cm_names` 是零依赖叶子，`cm_config` / `cm_types` 不得反向依赖 `cm_tools`。
# 这几条边此前只靠注释约束 —— Rust 单 crate 内模块循环引用照样编译，不加检查就没人拦得住。
check_forbidden_mod src/cm_names.rs "use crate::" \
  "cm_names is the zero-dependency leaf for builtin tool names"
check_forbidden_mod src/cm_config crate::cm_tools \
  "config stays below tools; tool names come from crate::cm_names"
check_forbidden_mod src/cm_types crate::cm_tools \
  "protocol contract stays below tools"

if [[ "$fail" -ne 0 ]]; then
  echo "dependency policy FAILED"
  exit 1
fi
echo "dependency policy OK"
