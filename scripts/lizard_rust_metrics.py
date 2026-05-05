#!/usr/bin/env python3
"""Rust 圈复杂度：单函数硬上限 + 两项棘轮基线（均不可恶化，降低时可自动写回）。

- 全体函数中 **最大 CCN** 不得高于 `scripts/lizard_max_ccn_baseline.txt`（可降低并自动更新文件）。
- **top10 CCN 之和** 不得高于 `scripts/lizard_top10_ccn_sum.txt`（可降低并自动更新文件）。

环境变量：
  LIZARD_CCN                  单函数 CCN 硬上限（默认 40），超过即失败
  LIZARD_TOP10_BASELINE_FILE  top10 之和基线文件路径
  LIZARD_MAX_BASELINE_FILE    最大 CCN 基线文件路径（默认 scripts/lizard_max_ccn_baseline.txt）
  LIZARD_NO_UPDATE_BASELINE   设为 1/true 时不写回基线；CI（CI=true）默认不写回
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src", ROOT / "crates", ROOT / "frontend" / "src"]
DEFAULT_TOP10_BASELINE = ROOT / "scripts" / "lizard_top10_ccn_sum.txt"
DEFAULT_MAX_BASELINE = ROOT / "scripts" / "lizard_max_ccn_baseline.txt"


def _rust_files() -> list[str]:
    out: list[str] = []
    for base in RUST_ROOTS:
        if base.is_dir():
            out.extend(str(p) for p in base.rglob("*.rs"))
    return out


def _truthy(s: str | None) -> bool:
    if s is None:
        return False
    return s.lower() in ("1", "true", "yes", "on")


def main() -> int:
    max_cap = int(os.environ.get("LIZARD_CCN", "40"))
    top10_baseline_path = Path(
        os.environ.get("LIZARD_TOP10_BASELINE_FILE", str(DEFAULT_TOP10_BASELINE))
    )
    max_baseline_path = Path(
        os.environ.get("LIZARD_MAX_BASELINE_FILE", str(DEFAULT_MAX_BASELINE))
    )
    no_update = _truthy(os.environ.get("LIZARD_NO_UPDATE_BASELINE"))
    if _truthy(os.environ.get("CI")):
        no_update = True

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
            if c > max_cap:
                over_cap.append((c, f.filename, fn.start_line, fn.name))

    if not ccns:
        print("lizard: 未分析到任何函数", file=sys.stderr)
        return 1

    overall_max = max(ccns)
    ccns_sorted = sorted(ccns, reverse=True)
    k = min(10, len(ccns_sorted))
    top10_sum = sum(ccns_sorted[:k])

    if top10_baseline_path.is_file():
        try:
            top10_baseline = int(top10_baseline_path.read_text().strip())
        except ValueError:
            print(
                f"lizard: 无法解析 top10 基线文件（应为整数一行）: {top10_baseline_path}",
                file=sys.stderr,
            )
            return 1
    else:
        top10_baseline = top10_sum
        if not no_update:
            top10_baseline_path.parent.mkdir(parents=True, exist_ok=True)
            top10_baseline_path.write_text(f"{top10_sum}\n", encoding="utf-8")
            print(
                f"lizard: 已创建 top10 CCN 之和基线 {top10_sum} -> {top10_baseline_path}",
                file=sys.stderr,
            )

    if max_baseline_path.is_file():
        try:
            max_baseline = int(max_baseline_path.read_text().strip())
        except ValueError:
            print(
                f"lizard: 无法解析最大 CCN 基线文件（应为整数一行）: {max_baseline_path}",
                file=sys.stderr,
            )
            return 1
    else:
        max_baseline = overall_max
        if not no_update:
            max_baseline_path.parent.mkdir(parents=True, exist_ok=True)
            max_baseline_path.write_text(f"{overall_max}\n", encoding="utf-8")
            print(
                f"lizard: 已创建最大 CCN 基线 {overall_max} -> {max_baseline_path}",
                file=sys.stderr,
            )

    print(
        f"lizard Rust: 函数数={len(ccns)}, max CCN={overall_max} "
        f"(硬上限≤{max_cap}, 棘轮基线≤{max_baseline}), "
        f"top{k} CCN 之和={top10_sum} (棘轮基线≤{top10_baseline})"
    )

    rc = 0
    if over_cap:
        rc = 1
        print(f"超过单函数 CCN 上限 ({max_cap})：", file=sys.stderr)
        over_cap.sort(key=lambda x: (-x[0], x[1], x[2], x[3]))
        for c, path, line, name in over_cap[:30]:
            print(f"  CCN {c}\t{path}:{line}\t{name}", file=sys.stderr)
        if len(over_cap) > 30:
            print(f"  ... 另有 {len(over_cap) - 30} 个", file=sys.stderr)

    if overall_max > max_baseline:
        print(
            f"lizard: 最大 CCN {overall_max} 高于棘轮基线 {max_baseline}（禁止拉高全局峰值）",
            file=sys.stderr,
        )
        rc = 1

    if top10_sum > top10_baseline:
        print(
            f"lizard: top{k} CCN 之和 {top10_sum} 高于棘轮基线 {top10_baseline}（禁止增加复杂度预算）",
            file=sys.stderr,
        )
        rc = 1

    if rc == 0 and not no_update:
        if overall_max < max_baseline:
            max_baseline_path.write_text(f"{overall_max}\n", encoding="utf-8")
            print(
                f"lizard: 已收紧最大 CCN 棘轮基线 {max_baseline} -> {overall_max} ({max_baseline_path})"
            )
        if top10_sum < top10_baseline:
            top10_baseline_path.write_text(f"{top10_sum}\n", encoding="utf-8")
            print(
                f"lizard: 已收紧 top10 CCN 之和棘轮基线 {top10_baseline} -> {top10_sum} ({top10_baseline_path})"
            )

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
