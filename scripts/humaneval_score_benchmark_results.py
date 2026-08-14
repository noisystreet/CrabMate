#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Score CrabMate `bench` HumanEval output using OpenAI HumanEval `check_correctness`.

Reads:
  * **tasks** JSONL in CrabMate format (must include `humaneval_test`, `entry_point`, `prompt`,
    `instance_id`; typically produced by `humaneval_official_to_crabmate_jsonl.py`).
  * **results** JSONL from `crabmate bench --benchmark human_eval` (`completion` field). When the
    bench is run with `--samples N`, the results contain `N` rows per `instance_id` (distinguished
    by `sample_index`), and this script aggregates them to report **pass@k**.

For each matching `instance_id`, runs the vendored ``execution.check_correctness`` (same logic as
upstream HumanEval). **Executes untrusted model-generated code** — run in a sandbox if exposed to
the public internet.

Usage::
    python3 scripts/humaneval_score_benchmark_results.py \\
        --tasks humaneval_crabmate_tasks.jsonl \\
        --results benchmark_results.jsonl \\
        --output humaneval_score.jsonl

Optional: ``--timeout 3.0`` (seconds per task), ``--k 10`` (max k for pass@k reporting).
"""

from __future__ import annotations

import argparse
import json
import sys
from math import comb
from pathlib import Path
from typing import Any, Dict, Iterator, List

# Vendored OpenAI HumanEval execution (see scripts/vendor/human_eval_openai/README.md).
_VENDOR = Path(__file__).resolve().parent / "vendor" / "human_eval_openai"
if not (_VENDOR / "execution.py").is_file():
    sys.exit(f"missing vendored HumanEval execution: {_VENDOR / 'execution.py'}")
sys.path.insert(0, str(_VENDOR.parent))

from human_eval_openai import execution  # type: ignore  # noqa: E402


def iter_jsonl(path: str) -> Iterator[Dict[str, Any]]:
    p = Path(path)
    with p.open(encoding="utf-8") as f:
        for lineno, line in enumerate(f, start=1):
            raw = line.strip()
            if not raw or raw.startswith("#"):
                continue
            try:
                yield json.loads(raw)
            except json.JSONDecodeError as e:
                raise SystemExit(f"{path}:{lineno}: invalid JSON: {e}") from e


def load_problems(tasks_path: str) -> Dict[str, Dict[str, Any]]:
    problems: Dict[str, Dict[str, Any]] = {}
    for row in iter_jsonl(tasks_path):
        iid = str(row.get("instance_id", "")).strip()
        if not iid:
            raise SystemExit(f"{tasks_path}: task missing instance_id")
        tid = str(row.get("task_id", iid)).strip()
        prompt = str(row.get("prompt", ""))
        entry = str(row.get("entry_point", "")).strip()
        test = str(row.get("humaneval_test", row.get("test", ""))).strip()
        if not entry or not test:
            raise SystemExit(
                f"{tasks_path}: task {iid!r} needs entry_point and humaneval_test for scoring"
            )
        problems[iid] = {
            "task_id": tid,
            "prompt": prompt,
            "entry_point": entry,
            "test": test,
        }
    return problems


def pass_at_k(n: int, c: int, k: int) -> float:
    """HumanEval 无偏估计：`1 - C(n-c, k) / C(n, k)`（要求 `k <= n`）。"""
    if k > n:
        raise ValueError(f"k={k} cannot exceed n={n}")
    # `math.comb(a, b)` 在 b > a 时返回 0，因此 n-c < k 时该式自然为 1.0。
    return 1.0 - comb(n - c, k) / comb(n, k)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tasks", required=True, help="CrabMate HumanEval task JSONL (with humaneval_test)")
    ap.add_argument("--results", required=True, help="benchmark_results.jsonl from crabmate bench")
    ap.add_argument(
        "--output",
        default="",
        help="Write per-task score JSONL here (default: <results>_humaneval_scores.jsonl)",
    )
    ap.add_argument("--timeout", type=float, default=3.0, help="Seconds for each check_correctness")
    ap.add_argument("--k", type=int, default=10, help="Max k for pass@k reporting (default 10)")
    args = ap.parse_args()

    problems = load_problems(args.tasks)
    out_path = args.output or f"{args.results}_humaneval_scores.jsonl"

    # instance_id -> {"n": samples, "c": passed, "task_id": str}
    agg: Dict[str, Dict[str, Any]] = {}
    scored = 0
    passed = 0
    human_eval_rows = 0
    skipped: List[str] = []
    missing_problem: List[str] = []

    out_p = Path(out_path)
    with out_p.open("w", encoding="utf-8") as out:
        for res in iter_jsonl(args.results):
            if str(res.get("benchmark", "")).strip() != "human_eval":
                continue
            human_eval_rows += 1
            iid = str(res.get("instance_id", "")).strip()
            if not iid:
                skipped.append("<empty instance_id>")
                continue
            sample_index = res.get("sample_index", 0)
            prob = problems.get(iid)
            if prob is None:
                missing_problem.append(iid)
                entry = agg.setdefault(iid, {"n": 0, "c": 0, "task_id": iid})
                entry["n"] += 1
                continue

            entry = agg.setdefault(iid, {"n": 0, "c": 0, "task_id": prob["task_id"]})
            entry["n"] += 1

            completion = res.get("completion")
            if completion is None or str(completion).strip() == "":
                row = {
                    "instance_id": iid,
                    "sample_index": sample_index,
                    "benchmark": "human_eval",
                    "bench_status": res.get("status"),
                    "skipped": True,
                    "reason": "empty_completion",
                }
                out.write(json.dumps(row, ensure_ascii=False) + "\n")
                skipped.append(iid)
                continue

            chk: Dict[str, Any] = execution.check_correctness(
                prob, str(completion), args.timeout, completion_id=None
            )
            scored += 1
            ok = bool(chk.get("passed"))
            if ok:
                passed += 1
                entry["c"] += 1
            row = {
                "instance_id": iid,
                "sample_index": sample_index,
                "benchmark": "human_eval",
                "bench_status": res.get("status"),
                "humaneval_passed": ok,
                "humaneval_result": chk.get("result"),
                "task_id": prob["task_id"],
            }
            out.write(json.dumps(row, ensure_ascii=False) + "\n")

    k_max = max(1, args.k)
    pass_at_k_table: Dict[str, float] = {}
    for k in range(1, k_max + 1):
        vals = [
            pass_at_k(entry["n"], entry["c"], k)
            for entry in agg.values()
            if entry["n"] >= k
        ]
        if vals:
            pass_at_k_table[f"pass@{k}"] = sum(vals) / len(vals)

    rate = (passed / scored) if scored else 0.0
    print(
        json.dumps(
            {
                "tasks_file": args.tasks,
                "results_file": args.results,
                "scores_file": out_path,
                "human_eval_rows_in_results": human_eval_rows,
                "scored_with_completion": scored,
                "passed": passed,
                "pass_rate": rate,
                "problems_with_samples": len(agg),
                "pass_at_k": pass_at_k_table,
                "skipped_empty_completion": len(skipped),
                "missing_task_definition": missing_problem,
            },
            indent=2,
            ensure_ascii=False,
        )
    )
    if missing_problem:
        print(
            "warning: results reference instance_id not found in --tasks:",
            ", ".join(missing_problem[:20])
            + (" …" if len(missing_problem) > 20 else ""),
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
