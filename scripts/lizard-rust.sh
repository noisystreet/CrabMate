#!/usr/bin/env bash
# 对 `src/`、`crates/` 与 `frontend/src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 未安装时：pip install lizard
#
# 规则（与 scripts/lizard_rust_metrics.py 一致）：
#   - 单函数 CCN 不得超过 LIZARD_CCN（默认 40）
#   - 全体函数中 **最大 CCN** 不得高于 scripts/lizard_max_ccn_baseline.txt；降低时自动写回（CI 不写回）
#   - **CCN > LIZARD_HIGH_CCN_SUM_THRESHOLD（默认 15）的全部函数，其 CCN 之和**
#     不得高于 scripts/lizard_high_ccn_sum_baseline.txt；降低时自动写回（CI 不写回）
#
# 环境变量：
#   LIZARD_CCN                         单函数上限（默认 40）
#   LIZARD_HIGH_CCN_SUM_THRESHOLD      高复杂度阈值（默认 15），严格大于该值的函数计入「之和」棘轮
#   LIZARD_HIGH_CCN_SUM_BASELINE_FILE  「高 CCN 之和」基线文件路径
#   LIZARD_MAX_BASELINE_FILE           最大 CCN 基线文件路径
#   LIZARD_NO_UPDATE_BASELINE          1/true 时不写回基线
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py"
