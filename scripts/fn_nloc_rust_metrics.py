#!/usr/bin/env python3
"""Rust 函数体行数（lizard `nloc`）：可选硬上限 + 两项棘轮基线（均不可恶化，降低时可自动写回）。

`nloc` 为 lizard 统计的非空代码行数（近似「函数有效行数」），与编辑器行号可能略有出入。

- 全体函数中 **最大 nloc** 不得高于 `scripts/fn_nloc_max_baseline.txt`。
- **nloc 最高的 10 个函数之和**不得高于 `scripts/fn_nloc_top10_sum_baseline.txt`。
- 若设置 **`FN_NLOC_CAP`**（正整数）：**单函数 nloc 超过该值即失败**（硬上限，不参与棘轮写回）。

环境变量：
  FN_NLOC_CAP                   可选；正整数时启用单函数行数硬上限
  FN_NLOC_MAX_BASELINE_FILE     最大 nloc 棘轮基线文件路径
  FN_NLOC_TOP10_BASELINE_FILE   top10 nloc 之和棘轮基线文件路径
  FN_NLOC_NO_UPDATE_BASELINE    设为 1/true 时不写回基线；CI（CI=true）默认不写回
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
RUST_ROOTS = [ROOT / "src", ROOT / "crates", ROOT / "frontend-leptos" / "src"]

DEFAULT_MAX_BASELINE = ROOT / "scripts" / "fn_nloc_max_baseline.txt"
DEFAULT_TOP10_BASELINE = ROOT / "scripts" / "fn_nloc_top10_sum_baseline.txt"


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


def _optional_positive_int(name: str) -> int | None:
    raw = os.environ.get(name)
    if raw is None or not str(raw).strip():
        return None
    v = int(str(raw).strip())
    if v <= 0:
        print(f"fn-nloc: 环境变量 {name} 须为正整数，收到 {raw!r}", file=sys.stderr)
        sys.exit(1)
    return v


def main() -> int:
    cap = _optional_positive_int("FN_NLOC_CAP")
    max_baseline_path = Path(
        os.environ.get("FN_NLOC_MAX_BASELINE_FILE", str(DEFAULT_MAX_BASELINE))
    )
    top10_baseline_path = Path(
        os.environ.get("FN_NLOC_TOP10_BASELINE_FILE", str(DEFAULT_TOP10_BASELINE))
    )
    no_update = _truthy(os.environ.get("FN_NLOC_NO_UPDATE_BASELINE"))
    if _truthy(os.environ.get("CI")):
        no_update = True

    files = _rust_files()
    if not files:
        print("fn-nloc: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    result = lizard.analyze_files(files)
    nlocs: list[int] = []
    over_cap: list[tuple[int, str, int, str]] = []
    for f in result:
        for fn in f.function_list:
            n = fn.nloc
            nlocs.append(n)
            if cap is not None and n > cap:
                over_cap.append((n, f.filename, fn.start_line, fn.name))

    if not nlocs:
        print("fn-nloc: 未分析到任何函数", file=sys.stderr)
        return 1

    overall_max = max(nlocs)
    nlocs_sorted = sorted(nlocs, reverse=True)
    k = min(10, len(nlocs_sorted))
    top10_sum = sum(nlocs_sorted[:k])

    def read_int(path: Path, create_val: int, label: str) -> int:
        if path.is_file():
            try:
                return int(path.read_text().strip())
            except ValueError:
                print(
                    f"fn-nloc: 无法解析基线文件（应为整数一行）: {path}",
                    file=sys.stderr,
                )
                sys.exit(1)
        if not no_update:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"{create_val}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已创建 {label} 基线 {create_val} -> {path}",
                file=sys.stderr,
            )
        return create_val

    max_baseline = read_int(max_baseline_path, overall_max, "最大 nloc")
    top10_baseline = read_int(top10_baseline_path, top10_sum, "top10 nloc 之和")

    cap_msg = f"硬上限={cap}" if cap is not None else "硬上限(未设 FN_NLOC_CAP)"
    print(
        f"fn-nloc Rust: 函数数={len(nlocs)}, max nloc={overall_max} "
        f"({cap_msg}, 棘轮≤{max_baseline}), "
        f"top{k} nloc 之和={top10_sum} (棘轮≤{top10_baseline})"
    )

    rc = 0
    if over_cap:
        rc = 1
        print(
            f"fn-nloc: 超过单函数行数硬上限 FN_NLOC_CAP={cap}（nloc）：",
            file=sys.stderr,
        )
        over_cap.sort(key=lambda x: (-x[0], x[1], x[2], x[3]))
        for n, path, line, name in over_cap[:40]:
            print(f"  nloc {n}\t{path}:{line}\t{name}", file=sys.stderr)
        if len(over_cap) > 40:
            print(f"  ... 另有 {len(over_cap) - 40} 个", file=sys.stderr)

    if overall_max > max_baseline:
        print(
            f"fn-nloc: 最大 nloc {overall_max} 高于棘轮基线 {max_baseline}（禁止拉高峰值）",
            file=sys.stderr,
        )
        rc = 1

    if top10_sum > top10_baseline:
        print(
            f"fn-nloc: top{k} nloc 之和 {top10_sum} 高于棘轮基线 {top10_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if rc == 0 and not no_update:
        if overall_max < max_baseline:
            max_baseline_path.write_text(f"{overall_max}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已收紧最大 nloc 棘轮 {max_baseline} -> {overall_max} ({max_baseline_path})"
            )
        if top10_sum < top10_baseline:
            top10_baseline_path.write_text(f"{top10_sum}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已收紧 top10 nloc 之和棘轮 {top10_baseline} -> {top10_sum} ({top10_baseline_path})"
            )

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
