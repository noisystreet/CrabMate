#!/usr/bin/env bash
# 对 `src/`、`crates/` 与 `frontend-leptos/src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 未安装时：pip install lizard
#
# 规则：
#   - 单函数 CCN 不得超过 LIZARD_CCN（默认 40）
#   - 全体函数中 **最大 CCN** 不得高于 scripts/lizard_max_ccn_baseline.txt；降低时自动写回（CI 不写回）
#   - **top10 CCN 之和** 不得高于 scripts/lizard_top10_ccn_sum.txt；降低时自动写回（CI 不写回）
#
# 环境变量：
#   LIZARD_CCN                  单函数上限（默认 40）
#   LIZARD_TOP10_BASELINE_FILE  top10 之和基线文件路径
#   LIZARD_MAX_BASELINE_FILE    最大 CCN 基线文件路径
#   LIZARD_NO_UPDATE_BASELINE   设为 1 禁止更新基线文件
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py"
