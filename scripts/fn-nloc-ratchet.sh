#!/usr/bin/env bash
# Rust 代码规模棘轮：函数体 nloc（lizard）+ 源文件物理行数，入口为 **`scripts/fn_nloc_rust_metrics.py`**（路径与行为写死在该脚本内）。
#
# 规则摘要：
#   - 最大 nloc ≤ scripts/fn_nloc_max_baseline.txt；top10 nloc 之和 ≤ scripts/fn_nloc_top10_sum_baseline.txt
#   - 单文件最大行数 ≤ scripts/rust_file_max_lines_baseline.txt；top10 文件行数和 ≤ scripts/rust_file_top10_lines_sum_baseline.txt
#   - 自动写回时：`rust_file_*` 两基线新值不得大于运行开始时磁盘上的值（棘轮禁止增大）
#   - Git：工作区基线不得大于 `git show HEAD` 中同路径（防抬基线过钩）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/fn_nloc_rust_metrics.py"
