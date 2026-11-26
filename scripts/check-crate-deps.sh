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
  # 注释行不算依赖（与 `check_no_cm_internal_refs` 同口径）：只剔除**整行**注释。
  # 行尾注释仍计入引用 —— 宁可多报，也别漏掉真实的 `use`。
  hits="$(grep -R --include='*.rs' -n -F "$needle" "$from_dir" 2>/dev/null \
    | grep -vE '(^|:)[0-9]+:[[:space:]]*(//|/\*)' || true)"
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
# #1510 P2a：`cm_kernel` 是工具层与领域层共用的中立内核 —— 两边都可以依赖它，
# 但它不能反向依赖任何一边（否则又变成谁都能被绕过的假中立）。
check_forbidden_mod src/cm_kernel.rs crate::cm_tools \
  "kernel is shared by tools and agent; tools must not be a dependency of it"
check_forbidden_mod src/cm_kernel.rs crate::cm_agent \
  "kernel is shared by tools and agent; agent must not be a dependency of it"
# #1510 P2b：领域层不再依赖工具层。`cm_agent` 曾引 `cm_tools::{tool_result, registry_policy,
# tool_naming, redact}` 共 11 处；tool_result 已拆成 `cm_tool_result`，其余进 `cm_kernel`。
# 注意：`cm_tool_result` 自身仍依赖 `cm_tools::workspace::path::WorkspacePathError`
# （安全敏感面，属 P3 `cm_tool_fs` 批次），所以这里只约束 `cm_agent`。
check_forbidden_mod src/cm_agent crate::cm_tools \
  "domain layer must not depend on the tool layer; shared types live in cm_tool_result / cm_kernel"
# `cm_tool_result` 目前仍依赖 `cm_tools::workspace::path::WorkspacePathError`（安全敏感面，
# 待 P3 `cm_tool_fs` 批次下沉），所以不能整体禁 `crate::cm_tools`；先堵住大头：
# 结果信封/解析不该反向依赖工具实现。
check_forbidden_mod src/cm_tool_result crate::cm_tools::tools \
  "tool_result must not depend on tool implementations; only workspace::path is a known exception"

# #1510 P0：`chat_job_queue` 是 `web` 的下游消费者 —— 只许 `web -> queue`，不许反向。
# 两者共享的类型定义在 `context::web_state_facet`；这条一旦报错，说明又有人把定义挪回 `web`
# 或直接在队列里 `use crate::web::…`（Rust 单 crate 内模块循环引用照样编译，只靠注释拦不住）。
check_forbidden_mod src/chat_job_queue crate::web:: \
  "queue must not reach back into web; shared types live in context::web_state_facet"

# #1510：端口层 `turn_runner` 只留 `TurnEngine` / `TurnStats` / `TurnOutcome`。
# 默认引擎（要调 `run_agent_turn`）在 `agent::default_turn_engine`；`StructuralCommit`
# 在 `cm_types`（`agent::model_context_view` 只是 re-export）。这两条一旦报错，
# 说明端口层又开始依赖具体实现。
check_forbidden_mod src/turn_runner.rs crate::run_agent_turn \
  "default engine lives in agent::default_turn_engine; port layer must not call the turn fn"
check_forbidden_mod src/turn_runner.rs agent::model_context_view \
  "StructuralCommit lives in cm_types; agent::model_context_view is only a re-export"
# #1510：端口层不得依赖实现层。错误类型（`RunAgentTurnError` 等）在 `turn_error`，
# 共享类型在 `cm_types` / `cm_agent`；这条一旦报错，说明端口层又在借 `agent` 的东西。
check_forbidden_mod src/turn_runner.rs crate::agent:: \
  "port layer must not depend on the implementation layer; types live in turn_error / cm_*"
# #1510：`turn_error` 是端口契约的错误类型，只能依赖 cm_* / text_util。
# `crate::llm` 是**实现层**别名模块（它自己 use 了 crate::agent），经它取类型等于绕回
# 实现层 —— 这层间接边禁边脚本的直接引用检查看不见，所以单独立一条。
check_forbidden_mod src/turn_error.rs crate::llm \
  "use crate::cm_llm (domain) instead of the crate::llm alias; the alias depends on agent"
check_forbidden_mod src/turn_error.rs crate::agent:: \
  "turn_error is a port contract; it must not depend on the implementation layer"

if [[ "$fail" -ne 0 ]]; then
  echo "dependency policy FAILED"
  exit 1
fi
echo "dependency policy OK"
