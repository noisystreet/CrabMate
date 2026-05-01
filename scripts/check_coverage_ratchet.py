#!/usr/bin/env python3
"""对照 fixtures/coverage_ratchet_baselines.json 校验 cargo llvm-cov JSON 摘要中的行覆盖率棘轮。

用法:
  python3 scripts/check_coverage_ratchet.py target/llvm-cov-summary.json

若某路径实际计入行数为 0（无匹配文件），脚本失败，避免静默跳过。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _aggregate_lines_percent(
    files: list[dict],
    repo: Path,
    pattern: str,
) -> tuple[float, int, int]:
    """返回 (line_percent, covered, count)。pattern：以 / 结尾则前缀匹配，否则精确匹配相对路径。"""
    prefix = pattern.endswith("/")
    tot_covered = 0
    tot_count = 0
    matched = 0
    for fi in files:
        raw = fi["filename"].replace("\\", "/")
        try:
            rel = str(Path(raw).resolve().relative_to(repo)).replace("\\", "/")
        except ValueError:
            continue
        if prefix:
            ok = rel.startswith(pattern)
        else:
            ok = rel == pattern
        if not ok:
            continue
        matched += 1
        lines = fi["summary"]["lines"]
        tot_covered += lines["covered"]
        tot_count += lines["count"]
    if matched == 0:
        raise ValueError(f"棘轮路径无匹配源文件: {pattern!r}")
    pct = 100.0 * tot_covered / tot_count if tot_count > 0 else 100.0
    return pct, tot_covered, tot_count


def main() -> int:
    repo = _repo_root()
    baseline_path = repo / "fixtures" / "coverage_ratchet_baselines.json"
    if len(sys.argv) != 2:
        print(f"用法: {sys.argv[0]} <llvm-cov-json-path>", file=sys.stderr)
        return 2
    cov_path = Path(sys.argv[1])
    if not cov_path.is_file():
        print(f"找不到覆盖率 JSON: {cov_path}", file=sys.stderr)
        return 2

    with baseline_path.open(encoding="utf-8") as f:
        baseline = json.load(f)
    targets = baseline["targets"]

    with cov_path.open(encoding="utf-8") as f:
        cov = json.load(f)
    files = cov["data"][0]["files"]

    failures: list[str] = []
    for t in targets:
        pattern = t["path"]
        minimum = float(t["min_line_percent"])
        pct, covered, count = _aggregate_lines_percent(files, repo, pattern)
        note = t.get("note", "")
        extra = f" ({note})" if note else ""
        if pct + 1e-9 < minimum:
            failures.append(
                f"{pattern}: 行覆盖率 {pct:.4f}% ({covered}/{count}) < 下限 {minimum}%{extra}"
            )
        else:
            print(
                f"OK {pattern}: {pct:.4f}% ({covered}/{count}) >= {minimum}%{extra}"
            )

    if failures:
        print("覆盖率棘轮未通过:", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
