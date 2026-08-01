#!/usr/bin/env bash
# 对 `src/`、`crates/` 与 `frontend/src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 未安装时：pip install lizard
#
# 规则（与 scripts/lizard_rust_metrics.py 一致）：按模块分别汇总；各模块独立 ccn_max
# 见 scripts/lizard_module_ccn_caps.toml（全局天花板 global_ccn_ceiling，默认 ≤15）。
# 额外参数原样传给 Python，例如：
#   bash scripts/lizard-rust.sh --list-modules
#   bash scripts/lizard-rust.sh --module crates/crabmate-tools
#   bash scripts/lizard-rust.sh --list-above 10
#   bash scripts/lizard-rust.sh --write-caps
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py" "$@"
