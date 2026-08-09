#!/usr/bin/env python3
"""Rust 圈复杂度：按模块统计 CCN 超阈函数个数（crabmate_agent）。

模块划分：
  - crates/<crate>
  - src/<顶层目录或文件>

门禁：各模块中 **CCN > high_ccn_threshold**（默认 10）的函数个数
必须 **恰好等于** caps 文件中该模块的上限：
  - 实测 > cap：失败（复杂度回潮，需拆分或有意抬高 cap）
  - 实测 < cap：失败（须主动调低 cap；可 `bash scripts/lizard-rust.sh --write-caps`）

配置见 **`scripts/lizard_module_ccn_caps.toml`**（`[modules]` + `default_over_max`）。
业务 UI 复杂度门禁在 Client 仓 crabmate-client。

用法：
  python3 scripts/lizard_rust_metrics.py
  python3 scripts/lizard_rust_metrics.py --module crates/crabmate-tools
  python3 scripts/lizard_rust_metrics.py --list-above 10
  python3 scripts/lizard_rust_metrics.py --write-caps
  bash scripts/lizard-rust.sh --module src/runtime --list-above 10
"""
from __future__ import annotations

import argparse
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src", ROOT / "crates"]
CAPS_PATH = ROOT / "scripts" / "lizard_module_ccn_caps.toml"


@dataclass
class CapsConfig:
    threshold: int
    default_over_max: int
    global_over_ceiling: int
    modules: dict[str, int]
    frozen_modules: frozenset[str] = frozenset()


@dataclass
class FnHit:
    ccn: int
    path: Path
    line: int
    name: str


@dataclass
class ModuleStats:
    fn_count: int = 0
    max_ccn: int = 0
    over_cap: int = 0
    over_threshold: list[FnHit] = field(default_factory=list)
    above_warn: list[FnHit] = field(default_factory=list)


def module_id_for(path: Path) -> str:
    """将源文件归入检查模块 id。"""
    try:
        rel = path.resolve().relative_to(ROOT)
    except ValueError:
        return str(path)
    parts = rel.parts
    if not parts:
        return str(rel)
    if parts[0] == "crates" and len(parts) >= 2:
        return f"crates/{parts[1]}"
    if parts[0] == "src":
        if len(parts) == 1:
            return "src"
        return f"src/{parts[1]}"
    return str(Path(*parts[:2]) if len(parts) >= 2 else rel)


def rust_files(*, only_module: str | None) -> list[Path]:
    out: list[Path] = []
    for base in RUST_ROOTS:
        if not base.is_dir():
            continue
        for p in base.rglob("*.rs"):
            if "target" in p.parts:
                continue
            if only_module is not None and module_id_for(p) != only_module:
                continue
            out.append(p)
    return out


def known_modules() -> list[str]:
    mods: set[str] = set()
    for base in RUST_ROOTS:
        if not base.is_dir():
            continue
        for p in base.rglob("*.rs"):
            if "target" in p.parts:
                continue
            mods.add(module_id_for(p))
    return sorted(mods)


def load_caps(path: Path = CAPS_PATH) -> CapsConfig:
    if not path.is_file():
        print(f"lizard: 缺少 caps 文件 {path}", file=sys.stderr)
        raise SystemExit(2)
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    # 旧版为「单函数 max CCN」caps（global_ccn_ceiling / default_ccn_max），不可当个数上限用
    if "high_ccn_threshold" not in raw and (
        "global_ccn_ceiling" in raw or "default_ccn_max" in raw
    ):
        print(
            "lizard: caps 仍为旧格式（单函数 ccn_max）。"
            "请执行: bash scripts/lizard-rust.sh --write-caps",
            file=sys.stderr,
        )
        raise SystemExit(2)
    threshold = int(raw.get("high_ccn_threshold", 10))
    if threshold < 1:
        print(f"lizard: high_ccn_threshold 无效: {threshold}", file=sys.stderr)
        raise SystemExit(2)
    default_over = int(raw.get("default_over_max", 0))
    if default_over < 0:
        print(f"lizard: default_over_max 无效: {default_over}", file=sys.stderr)
        raise SystemExit(2)
    ceiling = int(raw.get("global_over_ceiling", max(default_over, 10_000)))
    if ceiling < 0:
        print(f"lizard: global_over_ceiling 无效: {ceiling}", file=sys.stderr)
        raise SystemExit(2)
    if default_over > ceiling:
        print(
            f"lizard: default_over_max ({default_over}) > global_over_ceiling ({ceiling})",
            file=sys.stderr,
        )
        raise SystemExit(2)
    modules_raw = raw.get("modules") or {}
    if not isinstance(modules_raw, dict):
        print("lizard: [modules] 必须是表", file=sys.stderr)
        raise SystemExit(2)
    modules: dict[str, int] = {}
    for k, v in modules_raw.items():
        mid = str(k)
        cap = int(v)
        if cap < 0:
            print(
                f"lizard: 模块 {mid!r} 的 over{threshold}_max 无效: {cap}",
                file=sys.stderr,
            )
            raise SystemExit(2)
        if cap > ceiling:
            print(
                f"lizard: 模块 {mid!r} 的 over{threshold}_max={cap} "
                f"超过 global_over_ceiling={ceiling}",
                file=sys.stderr,
            )
            raise SystemExit(2)
        modules[mid] = cap
    frozen_raw = raw.get("frozen_modules") or []
    if not isinstance(frozen_raw, list):
        print("lizard: frozen_modules 必须是数组", file=sys.stderr)
        raise SystemExit(2)
    frozen = frozenset(str(x) for x in frozen_raw)
    unknown_frozen = sorted(frozen - set(modules))
    if unknown_frozen:
        print(
            "lizard: frozen_modules 含未在 [modules] 登记的键: "
            + ", ".join(unknown_frozen),
            file=sys.stderr,
        )
        raise SystemExit(2)
    return CapsConfig(threshold, default_over, ceiling, modules, frozen)


def cap_for(mid: str, caps: CapsConfig, *, missing: set[str]) -> int:
    if mid in caps.modules:
        return caps.modules[mid]
    missing.add(mid)
    return caps.default_over_max


def analyze(
    files: list[Path],
    caps: CapsConfig,
    *,
    list_above: int | None,
) -> tuple[dict[str, ModuleStats], set[str]]:
    by_mod: dict[str, ModuleStats] = defaultdict(ModuleStats)
    missing: set[str] = set()
    result = lizard.analyze_files([str(p) for p in files])
    for f in result:
        path = Path(f.filename)
        mid = module_id_for(path)
        st = by_mod[mid]
        st.over_cap = cap_for(mid, caps, missing=missing)
        for fn in f.function_list:
            c = int(fn.cyclomatic_complexity)
            st.fn_count += 1
            if c > st.max_ccn:
                st.max_ccn = c
            hit = FnHit(c, path, int(fn.start_line), fn.name)
            if c > caps.threshold:
                st.over_threshold.append(hit)
            if list_above is not None and c > list_above:
                st.above_warn.append(hit)
    return dict(by_mod), missing


def _rel(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


def print_module_table(
    by_mod: dict[str, ModuleStats],
    caps: CapsConfig,
    caps_path: Path,
) -> None:
    try:
        caps_rel = caps_path.resolve().relative_to(ROOT)
    except ValueError:
        caps_rel = caps_path
    the = caps.threshold
    print(
        f"lizard Rust（按模块限制 CCN>{the} 函数个数；"
        f"个数天花板≤{caps.global_over_ceiling}；配置 {caps_rel}）"
    )
    col = f">{the}"
    print(f"{'module':<36} {'fns':>6} {'max':>4} {col:>5} {'cap':>5}")
    print("-" * 62)
    total_fns = 0
    overall_max = 0
    total_over = 0
    total_cap = 0
    for mid in sorted(by_mod.keys()):
        st = by_mod[mid]
        n_over = len(st.over_threshold)
        total_fns += st.fn_count
        overall_max = max(overall_max, st.max_ccn)
        total_over += n_over
        total_cap += st.over_cap
        print(
            f"{mid:<36} {st.fn_count:>6} {st.max_ccn:>4} {n_over:>5} {st.over_cap:>5}"
        )
    print("-" * 62)
    print(
        f"{'TOTAL':<36} {total_fns:>6} {overall_max:>4} {total_over:>5} {total_cap:>5}"
    )


def print_hits(
    title: str,
    hits: list[FnHit],
    *,
    limit: int = 40,
    stream=None,
) -> None:
    if not hits:
        return
    out = sys.stderr if stream is None else stream
    hits = sorted(hits, key=lambda h: (-h.ccn, str(h.path), h.line, h.name))
    if title:
        print(title, file=out)
    for h in hits[:limit]:
        print(
            f"  CCN {h.ccn}\t{_rel(h.path)}:{h.line}\t{h.name}",
            file=out,
        )
    if len(hits) > limit:
        print(f"  ... 另有 {len(hits) - limit} 个", file=out)


def caps_file_header_lines(caps: CapsConfig) -> list[str]:
    the = caps.threshold
    lines = [
        "# 各模块「CCN > high_ccn_threshold」函数个数上限（lizard）。",
        "# 与 scripts/lizard_rust_metrics.py 配套。",
        "# - high_ccn_threshold：计入超标的 CCN 下限（默认 10，即统计 CCN>10）",
        "# - default_over_max：未在 [modules] 登记的新模块回退个数上限",
        "# - global_over_ceiling：任一模块配置的个数上限不得超过此值",
        "# - frozen_modules：禁止抬高 cap；实测变小时仍须下调（--write-caps 只降不升）",
        "# 棘轮：实测个数必须与 cap 一致；变小则 pre-commit / lizard 失败，须调低 cap。",
        "# 可用：python3 scripts/lizard_rust_metrics.py --write-caps 按当前实测个数重写",
        "",
        f"high_ccn_threshold = {the}",
        f"default_over_max = {caps.default_over_max}",
        f"global_over_ceiling = {caps.global_over_ceiling}",
    ]
    if caps.frozen_modules:
        frozen_list = ", ".join(f'"{m}"' for m in sorted(caps.frozen_modules))
        lines.append(f"frozen_modules = [{frozen_list}]")
    lines.extend(["", "[modules]"])
    return lines


def _cap_value_for_module(
    mid: str,
    measured: int,
    caps: CapsConfig,
) -> tuple[int, str | None]:
    """返回 (写入的 cap, 可选注释行)。frozen：只降不升。"""
    the = caps.threshold
    if mid not in caps.frozen_modules:
        return measured, None
    if mid not in caps.modules:
        print(
            f"lizard: frozen 模块 {mid!r} 缺少既有 cap，无法 --write-caps",
            file=sys.stderr,
        )
        raise SystemExit(2)
    value = min(caps.modules[mid], measured)
    comment = f"# 禁止抬高：{mid} CCN>{the} 函数个数上限（可随实测下调）"
    return value, comment


def write_caps_from_measured(
    by_mod: dict[str, ModuleStats],
    caps: CapsConfig,
    path: Path,
    *,
    preserve_unscanned: dict[str, int] | None = None,
) -> None:
    """按当前实测「CCN > threshold」个数写入 caps 文件的 [modules]。

    `frozen_modules`：允许下调到实测值，禁止抬高。
    `preserve_unscanned`：未参与本次扫描的模块保留原 cap（供 `--module` + `--write-caps` 合并，避免截断）。
    """
    the = caps.threshold
    lines = caps_file_header_lines(caps)
    values: dict[str, int] = {}
    comments: dict[str, str] = {}

    if preserve_unscanned:
        for mid, prev in preserve_unscanned.items():
            values[mid] = prev
            if mid in caps.frozen_modules:
                comments[mid] = (
                    f"# 禁止抬高：{mid} CCN>{the} 函数个数上限（可随实测下调）"
                )

    for mid in sorted(by_mod.keys()):
        if by_mod[mid].fn_count == 0:
            continue
        measured = max(
            0, min(len(by_mod[mid].over_threshold), caps.global_over_ceiling)
        )
        value, comment = _cap_value_for_module(mid, measured, caps)
        values[mid] = value
        if comment is not None:
            comments[mid] = comment
        elif mid in comments:
            del comments[mid]

    if not values:
        print("lizard: 没有可写入的模块 cap", file=sys.stderr)
        raise SystemExit(1)

    for mid in sorted(values.keys()):
        if mid in comments:
            lines.append(comments[mid])
        lines.append(f'"{mid}" = {values[mid]}')
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")
    try:
        rel = path.resolve().relative_to(ROOT)
    except ValueError:
        rel = path
    print(f"已写入 {rel}（{len(values)} 个模块）")


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="按模块检查 Rust 函数 CCN>阈值的个数（lizard）"
    )
    p.add_argument(
        "--module",
        metavar="ID",
        help="只检查一个模块，如 crates/crabmate-tools、src/runtime",
    )
    p.add_argument(
        "--list-modules",
        action="store_true",
        help="列出已知模块 id 后退出",
    )
    p.add_argument(
        "--list-above",
        type=int,
        metavar="N",
        help="额外列出各模块中 CCN>N 的函数（不改变硬上限失败条件）",
    )
    p.add_argument(
        "--write-caps",
        action="store_true",
        help=(
            "按当前实测各模块 CCN>阈值函数个数写入 lizard_module_ccn_caps.toml 后退出 0；"
            "与 --module 联用时合并更新该模块，保留其余 [modules] 条目"
        ),
    )
    p.add_argument(
        "--caps-file",
        type=Path,
        default=CAPS_PATH,
        help="caps TOML 路径（默认 scripts/lizard_module_ccn_caps.toml）",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    caps_path = (
        args.caps_file if args.caps_file.is_absolute() else ROOT / args.caps_file
    )

    if args.list_modules:
        for mid in known_modules():
            print(mid)
        return 0

    if args.module is not None:
        known = set(known_modules())
        if args.module not in known:
            print(
                f"未知模块 {args.module!r}。可用 --list-modules 查看。",
                file=sys.stderr,
            )
            return 2

    files = rust_files(only_module=args.module)
    if not files:
        print("lizard: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    if args.write_caps:
        if caps_path.is_file():
            raw_probe = tomllib.loads(caps_path.read_text(encoding="utf-8"))
            legacy = "high_ccn_threshold" not in raw_probe and (
                "global_ccn_ceiling" in raw_probe or "default_ccn_max" in raw_probe
            )
            if legacy:
                if args.module is not None:
                    print(
                        "lizard: caps 仍为旧格式；请先全量 "
                        "bash scripts/lizard-rust.sh --write-caps",
                        file=sys.stderr,
                    )
                    return 2
                caps = CapsConfig(10, 0, 10_000, {}, frozenset())
            else:
                caps = load_caps(caps_path)
        else:
            if args.module is not None:
                print(
                    "lizard: --write-caps 与 --module 联用时需要已有 caps 文件"
                    "（合并写入，避免截断其他模块）",
                    file=sys.stderr,
                )
                return 2
            caps = CapsConfig(10, 0, 10_000, {}, frozenset())
        by_mod, _ = analyze(files, caps, list_above=None)
        if not by_mod:
            print("lizard: 未分析到任何函数", file=sys.stderr)
            return 1
        preserve = None
        if args.module is not None:
            scanned = {mid for mid, st in by_mod.items() if st.fn_count > 0}
            preserve = {
                mid: cap
                for mid, cap in caps.modules.items()
                if mid not in scanned
            }
        write_caps_from_measured(
            by_mod, caps, caps_path, preserve_unscanned=preserve
        )
        return 0

    caps = load_caps(caps_path)
    by_mod, missing = analyze(files, caps, list_above=args.list_above)
    if not by_mod or sum(st.fn_count for st in by_mod.values()) == 0:
        print("lizard: 未分析到任何函数", file=sys.stderr)
        return 1

    print_module_table(by_mod, caps, caps_path)
    if missing:
        print(
            "lizard: 以下模块未在 caps 文件登记，已使用 default_over_max="
            f"{caps.default_over_max}: {', '.join(sorted(missing))}",
            file=sys.stderr,
        )

    if args.module is None:
        unused = sorted(set(caps.modules) - set(by_mod))
        if unused:
            print(
                "lizard: caps 中有未扫到的模块键（可清理）: "
                + ", ".join(unused),
                file=sys.stderr,
            )

    the = caps.threshold
    failed = False
    for mid in sorted(by_mod.keys()):
        st = by_mod[mid]
        n_over = len(st.over_threshold)
        if n_over > st.over_cap:
            failed = True
            print(
                f"[{mid}] CCN>{the} 函数个数 {n_over} 超过上限 {st.over_cap}：",
                file=sys.stderr,
            )
            print_hits("", st.over_threshold)
        elif n_over < st.over_cap:
            failed = True
            print(
                f"[{mid}] CCN>{the} 函数个数已降为 {n_over}，低于 cap {st.over_cap}；"
                "须主动调低 scripts/lizard_module_ccn_caps.toml 中该模块上限"
                "（或：bash scripts/lizard-rust.sh --write-caps）。",
                file=sys.stderr,
            )
        if args.list_above is not None and st.above_warn:
            print_hits(
                f"[{mid}] CCN > {args.list_above}：",
                st.above_warn,
                stream=sys.stdout,
            )

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
