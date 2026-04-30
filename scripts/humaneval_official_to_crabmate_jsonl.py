#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Convert OpenAI HumanEval official JSONL to CrabMate `BenchmarkTask` JSONL.

Official dataset lines look like::
    {"task_id": "HumanEval/0", "prompt": "...", "entry_point": "...", "canonical_solution": "...", "test": "..."}

CrabMate expects one JSON object per line with at least::
    instance_id, prompt, entry_point, humaneval_test

`humaneval_test` holds the official `test` field (not sent to the model during `bench`; used by
`scripts/humaneval_score_benchmark_results.py`).

Usage::
    python3 scripts/humaneval_official_to_crabmate_jsonl.py \\
        --input HumanEval.jsonl \\
        --output humaneval_crabmate_tasks.jsonl
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, Iterator


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


def to_crabmate_task(row: Dict[str, Any]) -> Dict[str, Any]:
    missing = [k for k in ("task_id", "prompt", "entry_point", "test") if k not in row]
    if missing:
        raise ValueError(f"HumanEval row missing keys {missing}; keys present: {sorted(row)}")

    task_id = str(row["task_id"])
    return {
        "instance_id": task_id,
        "task_id": task_id,
        "prompt": str(row["prompt"]),
        "entry_point": str(row["entry_point"]),
        "humaneval_test": str(row["test"]),
    }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input", required=True, help="Official HumanEval JSONL (e.g. HumanEval.jsonl)")
    p.add_argument("--output", required=True, help="Output path for CrabMate task JSONL")
    args = p.parse_args()

    n = 0
    out_p = Path(args.output)
    with out_p.open("w", encoding="utf-8") as out:
        for row in iter_jsonl(args.input):
            try:
                task = to_crabmate_task(row)
            except ValueError as e:
                raise SystemExit(f"{args.input}: {e}") from e
            out.write(json.dumps(task, ensure_ascii=False) + "\n")
            n += 1

    print(f"wrote {n} tasks to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
