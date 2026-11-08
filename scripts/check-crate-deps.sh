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

# S4 终态（#1300 §4.2 的「P3 收紧禁边」）：`cm_internal` 已由 P4 整体消解。
# 原先这里是 6 条按目录的否定边（cm_workflow / cm_tools / cm_agent / cm_approval /
# cm_web_host / cm_cli ↛ crate::cm_internal）。改成白名单式后规则只剩一条：
#   **任何目录都不许引用 `crate::cm_internal`，也不许重建 `src/cm_internal/`。**
# 前者比逐个目录列边更严（新目录自动被覆盖），后者挡住「以加工作区为名」的回归。
#
# 白名单当前为空：口径终态（C1）就是零引用，不存在「允许引用的目录」。
# 真需要豁免时往下面这个数组加**相对路径前缀**（如 `src/some_dir`），并在 PR 里写清理由
# —— 数组非空即意味着 `cm_internal` 正在回归，评审应当怀疑而不是默认接受。
CM_INTERNAL_ALLOW=()

check_no_cm_internal_refs() {
  if [[ -d src/cm_internal ]]; then
    echo "FORBIDDEN: src/cm_internal/ 不得重建（S4-P4 已消解该模块，见 #1300）"
    fail=1
  else
    echo "ok: src/cm_internal/ 不存在"
  fi

  # 注释行剔除：文档里写的路径不是引用（与已删除的 check-cm-internal-refs.sh 同口径）。
  local hits
  hits="$(grep -Rn --include='*.rs' 'crate::cm_internal' src 2>/dev/null \
    | grep -vE ':[0-9]+:[[:space:]]*(//|/\*)' || true)"
  if [[ -n "$hits" ]]; then
    if [[ ${#CM_INTERNAL_ALLOW[@]} -gt 0 ]]; then
      local filtered=""
      local line allow
      while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        for allow in "${CM_INTERNAL_ALLOW[@]}"; do
          [[ "$line" == "$allow"* ]] && continue 2
        done
        filtered+="$line"$'\n'
      done <<< "$hits"
      hits="$filtered"
    fi
  fi
  if [[ -n "$hits" ]]; then
    echo "FORBIDDEN: crate::cm_internal 已无白名单可依（S4 终态是零引用，见 #1300）"
    printf '%s\n' "$hits" | head -40
    fail=1
  else
    echo "ok: no crate::cm_internal references"
  fi
}
check_no_cm_internal_refs

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
