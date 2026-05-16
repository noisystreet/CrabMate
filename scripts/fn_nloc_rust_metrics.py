#!/usr/bin/env python3
"""Rust 代码规模检查（与 **`scripts/fn-nloc-ratchet.sh`** 同入口）：函数 `nloc` + 源文件物理行数。

- 全体函数 **最大 nloc** 不得高于 [`MAX_NLOC_CAP`]（200）。
- 全体已扫描 `.rs` **单文件最大物理行数** 不得高于 [`MAX_FILE_LINES_CAP`]（920）。
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
MAX_NLOC_CAP = 200
MAX_FILE_LINES_CAP = 920


def _rust_files() -> list[str]:
    return [
        str(p)
        for base in RUST_ROOTS
        if base.is_dir()
        for p in base.rglob("*.rs")
    ]


def _file_physical_line_count(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="replace") as f:
        return sum(1 for _ in f)


def _check_function_nloc(files: list[str]) -> int:
    max_nloc = 0
    fn_count = 0
    over_cap: list[tuple[int, str, int, str]] = []
    for f in lizard.analyze_files(files):
        for fn in f.function_list:
            fn_count += 1
            n = fn.nloc
            if n > max_nloc:
                max_nloc = n
            if n > MAX_NLOC_CAP:
                over_cap.append((n, f.filename, fn.start_line, fn.name))

    if fn_count == 0:
        print("fn-nloc: 未分析到任何函数", file=sys.stderr)
        return 1

    print(f"fn-nloc Rust: 函数数={fn_count}, max nloc={max_nloc} (上限≤{MAX_NLOC_CAP})")
    if max_nloc <= MAX_NLOC_CAP:
        return 0

    print(
        f"fn-nloc: 最大 nloc {max_nloc} 超过上限 {MAX_NLOC_CAP}",
        file=sys.stderr,
    )
    print("fn-nloc: 超标函数:", file=sys.stderr)
    for nloc, path, line, name in sorted(over_cap, key=lambda r: (-r[0], r[1], r[2])):
        print(f"  nloc={nloc}\t{path}:{line}\t{name}", file=sys.stderr)
    return 1


def _check_file_physical_lines(files: list[str]) -> int:
    file_max = 0
    file_count = 0
    over_cap: list[tuple[int, str]] = []
    for s in files:
        p = Path(s)
        try:
            nlines = _file_physical_line_count(p)
        except OSError as e:
            print(f"fn-nloc: 无法读取文件行数 {p}: {e}", file=sys.stderr)
            return 1
        file_count += 1
        if nlines > file_max:
            file_max = nlines
        if nlines > MAX_FILE_LINES_CAP:
            over_cap.append((nlines, s))

    if file_count == 0:
        print("fn-nloc: 未统计到任何源文件行数", file=sys.stderr)
        return 1

    print(
        f"fn-nloc Rust 文件: 文件数={file_count}, max 行数={file_max} "
        f"(上限≤{MAX_FILE_LINES_CAP})"
    )
    if file_max <= MAX_FILE_LINES_CAP:
        return 0

    print(
        f"fn-nloc: 单文件最大行数 {file_max} 超过上限 {MAX_FILE_LINES_CAP}",
        file=sys.stderr,
    )
    print("fn-nloc: 超标文件:", file=sys.stderr)
    for nlines, path in sorted(over_cap, key=lambda r: (-r[0], r[1])):
        print(f"  {nlines} 行\t{path}", file=sys.stderr)
    return 1


def main() -> int:
    files = _rust_files()
    if not files:
        print("fn-nloc: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    return _check_function_nloc(files) or _check_file_physical_lines(files)


if __name__ == "__main__":
    raise SystemExit(main())
