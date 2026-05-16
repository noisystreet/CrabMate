#!/usr/bin/env python3
"""Rust 函数形参个数上限（lizard `parameter_count`，含 `self`），与 **`scripts/fn-param-ratchet.sh`** 同入口。

全体函数 **最大形参个数** 不得高于 **9**（[`MAX_PARAM_CAP`]）。
"""
import sys
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = (ROOT / "src", ROOT / "crates", ROOT / "frontend" / "src")
MAX_PARAM_CAP = 9


def _rust_files() -> list[str]:
    return [
        str(p)
        for base in RUST_ROOTS
        if base.is_dir()
        for p in base.rglob("*.rs")
    ]


def main() -> int:
    files = _rust_files()
    if not files:
        print("fn-param: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    max_params = 0
    fn_count = 0
    over_cap: list[tuple[int, str, int, str]] = []
    for f in lizard.analyze_files(files):
        for fn in f.function_list:
            fn_count += 1
            p = fn.parameter_count
            if p > max_params:
                max_params = p
            if p > MAX_PARAM_CAP:
                over_cap.append((p, f.filename, fn.start_line, fn.name))

    if fn_count == 0:
        print("fn-param: 未分析到任何函数", file=sys.stderr)
        return 1

    print(
        f"fn-param Rust: 函数数={fn_count}, max 形参={max_params} (上限≤{MAX_PARAM_CAP})"
    )
    if max_params <= MAX_PARAM_CAP:
        return 0

    print(
        f"fn-param: 最大形参 {max_params} 超过上限 {MAX_PARAM_CAP}",
        file=sys.stderr,
    )
    print("fn-param: 超标函数:", file=sys.stderr)
    for n, path, line, name in sorted(over_cap, key=lambda r: (-r[0], r[1], r[2])):
        print(f"  {n} 个形参\t{path}:{line}\t{name}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
