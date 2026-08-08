#!/usr/bin/env python3
"""Rust 圈复杂度：按模块分别汇总与硬上限。

模块划分：
  - crates/<crate>
  - src/<顶层目录或文件>

各模块上限见 **`scripts/lizard_module_ccn_caps.toml`**（`[modules]` + `default_ccn_max`）。
全局天花板 **`global_ccn_ceiling`**（默认 15）：配置中的模块 cap 不得高于此值。

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
    global_ceiling: int
    default_max: int
    modules: dict[str, int]


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
    cap: int = 0
    over_cap: list[FnHit] = field(default_factory=list)
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
            mods.add(module_id_for(p))
    return sorted(mods)


def load_caps(path: Path = CAPS_PATH) -> CapsConfig:
    if not path.is_file():
        print(f"lizard: 缺少 caps 文件 {path}", file=sys.stderr)
        raise SystemExit(2)
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    ceiling = int(raw.get("global_ccn_ceiling", 15))
    default_max = int(raw.get("default_ccn_max", ceiling))
    if default_max > ceiling:
        print(
            f"lizard: default_ccn_max ({default_max}) > global_ccn_ceiling ({ceiling})",
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
        if cap < 1:
            print(f"lizard: 模块 {mid!r} 的 ccn_max 无效: {cap}", file=sys.stderr)
            raise SystemExit(2)
        if cap > ceiling:
            print(
                f"lizard: 模块 {mid!r} 的 ccn_max={cap} 超过 global_ccn_ceiling={ceiling}",
                file=sys.stderr,
            )
            raise SystemExit(2)
        modules[mid] = cap
    return CapsConfig(ceiling, default_max, modules)


def cap_for(mid: str, caps: CapsConfig, *, missing: set[str]) -> int:
    if mid in caps.modules:
        return caps.modules[mid]
    missing.add(mid)
    return caps.default_max


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
        mod_cap = cap_for(mid, caps, missing=missing)
        st.cap = mod_cap
        for fn in f.function_list:
            c = int(fn.cyclomatic_complexity)
            st.fn_count += 1
            if c > st.max_ccn:
                st.max_ccn = c
            hit = FnHit(c, path, int(fn.start_line), fn.name)
            if c > mod_cap:
                st.over_cap.append(hit)
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
    print(
        "lizard Rust（按模块独立 ccn_max；"
        f"全局天花板≤{caps.global_ceiling}；配置 {caps_rel}）"
    )
    print(f"{'module':<36} {'fns':>6} {'max':>4} {'cap':>4} {'>cap':>5}")
    print("-" * 60)
    total_fns = 0
    overall_max = 0
    total_over = 0
    for mid in sorted(by_mod.keys()):
        st = by_mod[mid]
        total_fns += st.fn_count
        overall_max = max(overall_max, st.max_ccn)
        total_over += len(st.over_cap)
        print(
            f"{mid:<36} {st.fn_count:>6} {st.max_ccn:>4} {st.cap:>4} {len(st.over_cap):>5}"
        )
    print("-" * 60)
    print(
        f"{'TOTAL':<36} {total_fns:>6} {overall_max:>4} {'':>4} {total_over:>5}"
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
    print(title, file=out)
    for h in hits[:limit]:
        print(
            f"  CCN {h.ccn}\t{_rel(h.path)}:{h.line}\t{h.name}",
            file=out,
        )
    if len(hits) > limit:
        print(f"  ... 另有 {len(hits) - limit} 个", file=out)


def write_caps_from_measured(
    by_mod: dict[str, ModuleStats],
    caps: CapsConfig,
    path: Path,
) -> None:
    """按当前实测 max（夹在 1..ceiling）重写 caps 文件的 [modules]。"""
    lines = [
        "# 各模块单函数 CCN 硬上限（lizard）。与 scripts/lizard_rust_metrics.py 配套。",
        "# - global_ccn_ceiling：任一模块的 cap 不得超过此值（仓库全局天花板）",
        "# - default_ccn_max：未在 [modules] 登记的新模块回退值",
        "# 收紧：重构后把对应模块数值调低；升高须有意为之（勿默认放宽）。",
        "# 可用：python3 scripts/lizard_rust_metrics.py --write-caps 按当前实测 max 重写 [modules]",
        "",
        f"global_ccn_ceiling = {caps.global_ceiling}",
        f"default_ccn_max = {caps.default_max}",
        "",
        "[modules]",
    ]
    n = 0
    for mid in sorted(by_mod.keys()):
        if by_mod[mid].fn_count == 0:
            continue
        measured = max(1, min(by_mod[mid].max_ccn, caps.global_ceiling))
        lines.append(f'"{mid}" = {measured}')
        n += 1
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")
    try:
        rel = path.resolve().relative_to(ROOT)
    except ValueError:
        rel = path
    print(f"已写入 {rel}（{n} 个模块）")


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="按模块检查 Rust 函数 CCN（lizard；独立 ccn_max）"
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
        help="按当前实测各模块 max CCN 重写 lizard_module_ccn_caps.toml 后退出 0",
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
            caps = load_caps(caps_path)
        else:
            caps = CapsConfig(15, 15, {})
        by_mod, _ = analyze(files, caps, list_above=None)
        if not by_mod:
            print("lizard: 未分析到任何函数", file=sys.stderr)
            return 1
        write_caps_from_measured(by_mod, caps, caps_path)
        return 0

    caps = load_caps(caps_path)
    by_mod, missing = analyze(files, caps, list_above=args.list_above)
    if not by_mod or sum(st.fn_count for st in by_mod.values()) == 0:
        print("lizard: 未分析到任何函数", file=sys.stderr)
        return 1

    print_module_table(by_mod, caps, caps_path)
    if missing:
        print(
            "lizard: 以下模块未在 caps 文件登记，已使用 default_ccn_max="
            f"{caps.default_max}: {', '.join(sorted(missing))}",
            file=sys.stderr,
        )

    # 全量扫描时提示 caps 中多余键；单模块模式跳过
    if args.module is None:
        unused = sorted(set(caps.modules) - set(by_mod))
        if unused:
            print(
                "lizard: caps 中有未扫到的模块键（可清理）: "
                + ", ".join(unused),
                file=sys.stderr,
            )

    failed = False
    for mid in sorted(by_mod.keys()):
        st = by_mod[mid]
        if st.over_cap:
            failed = True
            print_hits(
                f"[{mid}] 超过该模块 ccn_max ({st.cap})：",
                st.over_cap,
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
