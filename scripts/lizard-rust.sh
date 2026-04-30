#!/usr/bin/env bash
# 对 `src/` 与 `crates/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 未安装时：pip install lizard
#
# 环境变量 LIZARD_CCN：超过该 CCN 的函数视为 warning 并以非零退出（默认 298，与当前仓库中最高 CCN 函数对齐，避免 baseline 全红）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
THRESHOLD="${LIZARD_CCN:-298}"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
# -l rust：仅 Rust；-C：CCN 上限（超过则报 warning 且非零退出）；-w：仅输出警告
exec python3 -m lizard src crates -l rust -C "$THRESHOLD" -w
