#!/usr/bin/env python3
"""Rust 圈复杂度门禁：`src/` 下每个函数 CCN 不得超过 **CCN_MAX**（默认 10）。

不再按模块计数、也不再维护 caps TOML。业务 UI 复杂度门禁在 Client 仓。

用法：
  python3 scripts/lizard_rust_metrics.py
  python3 scripts/lizard_rust_metrics.py --list-above 8
  bash scripts/lizard-rust.sh
"""
from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src"]
# 允许的最大 CCN（含）；CCN > CCN_MAX 失败。
CCN_MAX = 10


@dataclass(frozen=True)
class FnHit:
    ccn: int
    path: Path
    line: int
    name: str


def rust_files() -> list[Path]:
    out: list[Path] = []
    for base in RUST_ROOTS:
        if not base.is_dir():
            continue
        for p in base.rglob("*.rs"):
            if "target" in p.parts:
                continue
            out.append(p)
    return out


def _rel(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


def collect_functions(files: list[Path]) -> list[FnHit]:
    hits: list[FnHit] = []
    result = lizard.analyze_files([str(p) for p in files])
    for f in result:
        path = Path(f.filename)
        for fn in f.function_list:
            hits.append(
                FnHit(
                    int(fn.cyclomatic_complexity),
                    path,
                    int(fn.start_line),
                    fn.name,
                )
            )
    return hits


def print_hits(title: str, hits: list[FnHit], *, stream, limit: int = 80) -> None:
    if not hits:
        return
    hits = sorted(hits, key=lambda h: (-h.ccn, str(h.path), h.line, h.name))
    print(title, file=stream)
    for h in hits[:limit]:
        print(f"  CCN {h.ccn}\t{_rel(h.path)}:{h.line}\t{h.name}", file=stream)
    if len(hits) > limit:
        print(f"  ... 另有 {len(hits) - limit} 个", file=stream)


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="检查 src/ 下 Rust 函数 CCN 是否均 ≤ 全局上限（默认 10）"
    )
    p.add_argument(
        "--list-above",
        type=int,
        metavar="N",
        help="额外列出 CCN>N 的函数（不改变硬上限失败条件）",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    files = rust_files()
    if not files:
        print("lizard: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    fns = collect_functions(files)
    if not fns:
        print("lizard: 未分析到任何函数", file=sys.stderr)
        return 1

    max_ccn = max(h.ccn for h in fns)
    over = [h for h in fns if h.ccn > CCN_MAX]
    print(
        f"lizard Rust（全局 CCN≤{CCN_MAX}；"
        f"函数 {len(fns)}；实测 max={max_ccn}；CCN>{CCN_MAX} 个数 {len(over)}）"
    )

    if args.list_above is not None:
        print_hits(
            f"CCN > {args.list_above}：",
            [h for h in fns if h.ccn > args.list_above],
            stream=sys.stdout,
        )

    if over:
        print_hits(
            f"lizard: {len(over)} 个函数 CCN>{CCN_MAX}（须拆分至 ≤{CCN_MAX}）：",
            over,
            stream=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
