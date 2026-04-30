#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Score CrabMate `bench` HumanEval output using OpenAI HumanEval `check_correctness`.

Reads:
  * **tasks** JSONL in CrabMate format (must include `humaneval_test`, `entry_point`, `prompt`,
    `instance_id`; typically produced by `humaneval_official_to_crabmate_jsonl.py`).
  * **results** JSONL from `crabmate bench --benchmark human_eval` (`completion` field).

For each matching `instance_id`, runs the vendored ``execution.check_correctness`` (same logic as
upstream HumanEval). **Executes untrusted model-generated code** — run in a sandbox if exposed to
the public internet.

Usage::
    python3 scripts/humaneval_score_benchmark_results.py \\
        --tasks humaneval_crabmate_tasks.jsonl \\
        --results benchmark_results.jsonl \\
        --output humaneval_score.jsonl

Optional: ``--timeout 3.0`` (seconds per task, forwarded to ``check_correctness``).
"""

from __future__ import annotations

import argparse
import json
import sys
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
    args = ap.parse_args()

    problems = load_problems(args.tasks)
    out_path = args.output or f"{args.results}_humaneval_scores.jsonl"

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
            prob = problems.get(iid)
            if prob is None:
                missing_problem.append(iid)
                continue
            completion = res.get("completion")
            if completion is None or str(completion).strip() == "":
                row = {
                    "instance_id": iid,
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
            if chk.get("passed"):
                passed += 1
            row = {
                "instance_id": iid,
                "benchmark": "human_eval",
                "bench_status": res.get("status"),
                "humaneval_passed": chk.get("passed"),
                "humaneval_result": chk.get("result"),
                "task_id": prob["task_id"],
            }
            out.write(json.dumps(row, ensure_ascii=False) + "\n")

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
