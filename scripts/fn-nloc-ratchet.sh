#!/usr/bin/env bash
# Rust 函数体行数（lizard nloc）棘轮，与 scripts/fn_nloc_rust_metrics.py 对齐。
#
# 规则：
#   - 最大 nloc 不得高于 scripts/fn_nloc_max_baseline.txt
#   - top10 nloc 之和不得高于 scripts/fn_nloc_top10_sum_baseline.txt
#   - 可选：FN_NLOC_CAP=100 启用单函数行数硬上限
#
# 环境变量：见 fn_nloc_rust_metrics.py 文件头注释。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/fn_nloc_rust_metrics.py"
