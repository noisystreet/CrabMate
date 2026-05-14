#!/usr/bin/env python3
"""Rust 圈复杂度：单函数 CCN 不得超过 15。"""
from __future__ import annotations

import sys
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src", ROOT / "crates", ROOT / "frontend" / "src"]
MAX_CAP = 15


def _rust_files() -> list[str]:
    out: list[str] = []
    for base in RUST_ROOTS:
        if base.is_dir():
            out.extend(str(p) for p in base.rglob("*.rs"))
    return out


def main() -> int:
    files = _rust_files()
    if not files:
        print("lizard: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    result = lizard.analyze_files(files)
    ccns: list[int] = []
    over_cap: list[tuple[int, str, int, str]] = []
    for f in result:
        for fn in f.function_list:
            c = fn.cyclomatic_complexity
            ccns.append(c)
            if c > MAX_CAP:
                over_cap.append((c, f.filename, fn.start_line, fn.name))

    if not ccns:
        print("lizard: 未分析到任何函数", file=sys.stderr)
        return 1

    overall_max = max(ccns)
    print(f"lizard Rust: 函数数={len(ccns)}, max CCN={overall_max} (硬上限≤{MAX_CAP})")

    if not over_cap:
        return 0

    print(f"超过单函数 CCN 上限 ({MAX_CAP})：", file=sys.stderr)
    over_cap.sort(key=lambda x: (-x[0], x[1], x[2], x[3]))
    for c, path, line, name in over_cap[:30]:
        print(f"  CCN {c}\t{path}:{line}\t{name}", file=sys.stderr)
    if len(over_cap) > 30:
        print(f"  ... 另有 {len(over_cap) - 30} 个", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
