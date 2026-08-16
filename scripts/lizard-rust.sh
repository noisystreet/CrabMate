#!/usr/bin/env bash
# 对 `src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 业务 UI 复杂度门禁在 Client 仓 crabmate-client（同为「CCN>10 函数个数」棘轮）。
# 未安装时：pip install lizard
#
# 规则（与 scripts/lizard_rust_metrics.py 一致）：
# 1) 按模块统计 CCN>10（可配）的函数个数，各模块上限见 lizard_module_ccn_caps.toml；
# 2) 全量扫描时另卡「超阈函数 CCN 之和」= global_over_ccn_sum_cap。
# 棘轮：实测必须等于 cap；变小则检查失败，须调低 cap（--write-caps）。
# --write-caps 与 --module 联用时合并更新该模块个数，不重算全局之和。
# 额外参数原样传给 Python，例如：
#   bash scripts/lizard-rust.sh --list-modules
#   bash scripts/lizard-rust.sh --module src/cm_tools
#   bash scripts/lizard-rust.sh --list-above 10
#   bash scripts/lizard-rust.sh --write-caps
#   bash scripts/lizard-rust.sh --module src/runtime --write-caps
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py" "$@"
