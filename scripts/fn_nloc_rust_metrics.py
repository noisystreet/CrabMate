#!/usr/bin/env python3
"""Rust 代码规模棘轮（与 **`scripts/fn-nloc-ratchet.sh`** 同入口）：函数 `nloc` + 源文件物理行数。

**A. 函数体行数（lizard `nloc`）**

`nloc` 为 lizard 统计的非空代码行数（近似「函数有效行数」），与编辑器行号可能略有出入。

- 全体函数中 **最大 nloc** 不得高于 `scripts/fn_nloc_max_baseline.txt`。
- **nloc 最高的 10 个函数之和**不得高于 `scripts/fn_nloc_top10_sum_baseline.txt`。
- 若设置 **`FN_NLOC_CAP`**（正整数）：**单函数 nloc 超过该值即失败**（硬上限，不参与棘轮写回）。

**B. 源文件物理行数（按 `.rs` 文件）**

- 全体已扫描 Rust 源文件中 **最大行数**不得高于 `scripts/rust_file_max_lines_baseline.txt`。
- **行数最高的 10 个文件之行数和**不得高于 `scripts/rust_file_top10_lines_sum_baseline.txt`。
- 若设置 **`RUST_FILE_LINES_MAX_CAP`**（正整数）：**单行数超过该值的文件即失败**（硬上限，不参与棘轮写回）。

环境变量：
  FN_NLOC_CAP                   可选；单函数 nloc 硬上限
  FN_NLOC_MAX_BASELINE_FILE     最大 nloc 棘轮基线文件路径
  FN_NLOC_TOP10_BASELINE_FILE   top10 nloc 之和棘轮基线文件路径
  RUST_FILE_LINES_MAX_CAP       可选；单文件物理行数硬上限
  RUST_FILE_MAX_LINES_BASELINE_FILE       单文件最大行数棘轮基线路径
  RUST_FILE_TOP10_LINES_SUM_BASELINE_FILE top10 文件行数和棘轮基线路径
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
DEFAULT_FILE_MAX_BASELINE = ROOT / "scripts" / "rust_file_max_lines_baseline.txt"
DEFAULT_FILE_TOP10_BASELINE = ROOT / "scripts" / "rust_file_top10_lines_sum_baseline.txt"


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


def _read_int(path: Path, create_val: int, label: str, *, no_update: bool) -> int:
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


def _file_physical_line_count(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="replace") as f:
        return sum(1 for _ in f)


def main() -> int:
    cap = _optional_positive_int("FN_NLOC_CAP")
    file_cap = _optional_positive_int("RUST_FILE_LINES_MAX_CAP")
    max_baseline_path = Path(
        os.environ.get("FN_NLOC_MAX_BASELINE_FILE", str(DEFAULT_MAX_BASELINE))
    )
    top10_baseline_path = Path(
        os.environ.get("FN_NLOC_TOP10_BASELINE_FILE", str(DEFAULT_TOP10_BASELINE))
    )
    file_max_baseline_path = Path(
        os.environ.get(
            "RUST_FILE_MAX_LINES_BASELINE_FILE", str(DEFAULT_FILE_MAX_BASELINE)
        )
    )
    file_top10_baseline_path = Path(
        os.environ.get(
            "RUST_FILE_TOP10_LINES_SUM_BASELINE_FILE",
            str(DEFAULT_FILE_TOP10_BASELINE),
        )
    )
    no_update = _truthy(os.environ.get("FN_NLOC_NO_UPDATE_BASELINE"))
    if _truthy(os.environ.get("CI")):
        no_update = True

    files = _rust_files()
    if not files:
        print("fn-nloc: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    # --- function nloc (lizard) ---
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
    k_fn = min(10, len(nlocs_sorted))
    top10_sum = sum(nlocs_sorted[:k_fn])

    max_baseline = _read_int(
        max_baseline_path, overall_max, "最大 nloc", no_update=no_update
    )
    top10_baseline = _read_int(
        top10_baseline_path, top10_sum, "top10 nloc 之和", no_update=no_update
    )

    cap_msg = f"硬上限={cap}" if cap is not None else "硬上限(未设 FN_NLOC_CAP)"
    print(
        f"fn-nloc Rust: 函数数={len(nlocs)}, max nloc={overall_max} "
        f"({cap_msg}, 棘轮≤{max_baseline}), "
        f"top{k_fn} nloc 之和={top10_sum} (棘轮≤{top10_baseline})"
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
            f"fn-nloc: top{k_fn} nloc 之和 {top10_sum} 高于棘轮基线 {top10_baseline}",
            file=sys.stderr,
        )
        rc = 1

    # --- per-file physical line counts ---
    file_counts: list[tuple[int, str]] = []
    over_file_cap: list[tuple[int, str]] = []
    for s in files:
        p = Path(s)
        try:
            nlines = _file_physical_line_count(p)
        except OSError as e:
            print(f"fn-nloc: 无法读取文件行数 {p}: {e}", file=sys.stderr)
            rc = 1
            continue
        file_counts.append((nlines, s))
        if file_cap is not None and nlines > file_cap:
            over_file_cap.append((nlines, s))

    if not file_counts:
        print("fn-nloc: 未统计到任何源文件行数", file=sys.stderr)
        return 1

    file_max = max(c[0] for c in file_counts)
    file_counts_sorted = sorted(file_counts, key=lambda x: -x[0])
    k_file = min(10, len(file_counts_sorted))
    file_top10_sum = sum(c[0] for c in file_counts_sorted[:k_file])

    file_max_baseline = _read_int(
        file_max_baseline_path, file_max, "单文件最大行数", no_update=no_update
    )
    file_top10_baseline = _read_int(
        file_top10_baseline_path,
        file_top10_sum,
        "top10 文件行数之和",
        no_update=no_update,
    )

    fcap_msg = (
        f"硬上限={file_cap}"
        if file_cap is not None
        else "硬上限(未设 RUST_FILE_LINES_MAX_CAP)"
    )
    print(
        f"fn-nloc Rust 文件: 文件数={len(file_counts)}, max 行数={file_max} "
        f"({fcap_msg}, 棘轮≤{file_max_baseline}), "
        f"top{k_file} 文件行数和={file_top10_sum} (棘轮≤{file_top10_baseline})"
    )

    if over_file_cap:
        rc = 1
        print(
            f"fn-nloc: 超过单文件行数硬上限 RUST_FILE_LINES_MAX_CAP={file_cap}：",
            file=sys.stderr,
        )
        over_file_cap.sort(key=lambda x: (-x[0], x[1]))
        for nlines, path in over_file_cap[:40]:
            print(f"  lines {nlines}\t{path}", file=sys.stderr)
        if len(over_file_cap) > 40:
            print(f"  ... 另有 {len(over_file_cap) - 40} 个", file=sys.stderr)

    if file_max > file_max_baseline:
        print(
            f"fn-nloc: 单文件最大行数 {file_max} 高于棘轮基线 {file_max_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if file_top10_sum > file_top10_baseline:
        print(
            f"fn-nloc: top{k_file} 文件行数和 {file_top10_sum} 高于棘轮基线 {file_top10_baseline}",
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
        if file_max < file_max_baseline:
            file_max_baseline_path.write_text(f"{file_max}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已收紧单文件最大行数棘轮 {file_max_baseline} -> {file_max} ({file_max_baseline_path})"
            )
        if file_top10_sum < file_top10_baseline:
            file_top10_baseline_path.write_text(
                f"{file_top10_sum}\n", encoding="utf-8"
            )
            print(
                f"fn-nloc: 已收紧 top10 文件行数和棘轮 {file_top10_baseline} -> {file_top10_sum} ({file_top10_baseline_path})"
            )

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
