#!/usr/bin/env bash
# Rust 函数形参个数棘轮（lizard parameter_count + allow 行数），入口为 **`scripts/fn_param_rust_metrics.py`**（硬上限与基线路径写死在该脚本内）。
#
# 规则：
#   - 单函数形参个数不得超过 **32**（脚本内常量）
#   - 最大形参不得高于 scripts/fn_param_max_baseline.txt
#   - top10 形参之和不得高于 scripts/fn_param_top10_sum_baseline.txt
#   - #[allow(clippy::too_many_arguments)] 行数不得高于 scripts/fn_param_allow_count_baseline.txt
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（fn-param 棘轮与 lizard-rust 共用 lizard）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/fn_param_rust_metrics.py"
