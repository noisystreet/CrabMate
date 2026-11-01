#!/usr/bin/env bash
# 生产路径 unwrap/expect ratchet（#1152）：把「裸 unwrap / expect」的总量冻在基线，
# 只许降不许升。实现与口径说明见 **`scripts/unwrap_ratchet.py`**；
# 基线 `scripts/unwrap_baseline.txt` 只许降不许升。
#
# 口径走 clippy（`--lib --all-features`，不含 test cfg）而不是静态扫描：本仓库重度
# 使用 `#[cfg(feature = ...)]`，静态扫描分不清「被 feature 关掉的模块」与
# 「#[cfg(test)] 里的用法」，实测会差 3 处。详见脚本 docstring。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/scripts/unwrap_ratchet.py"
