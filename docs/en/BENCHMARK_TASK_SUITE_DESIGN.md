# Benchmark task suite design (CrabMate)

**Status**: Design note for how we **curate and operate** evaluation task suites—not a commitment to procure any specific commercial dataset.  
**Audience**: Maintainers, eval owners, product, CI owners.  
**Language**: English.  
**Related docs**: **`docs/基准测试规划.md`** (Chinese: `crabmate bench`, adapters), **`benchmark/README.md`**, **`docs/en/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`**, **`docs/en/TESTING.md`**.

---

## 1. Goals

Make “automatically test whether the agent completes real tasks and collect evidence for improvement” **maintainable**:

- Prefer **machine-checkable** success criteria.
- Pin **workspace + toolchain** expectations.
- Control **cost, secrets, and flakiness**.
- Align tasks with **product surfaces** we care about (staged planning, `acceptance`, workflows, approvals, etc.).

Field-level JSONL contracts remain authoritative in **`src/runtime/benchmark/types.rs`** and **`docs/基准测试规划.md`**.

---

## 2. Design dimensions (checklist)

| Dimension | Expectation |
|-----------|-------------|
| **Measurability** | Exit codes, tests, file predicates, structured JSON—not “LLM-as-judge only”. |
| **Reproducibility** | Fixed workspace snapshots; declare OS/tool deps; optionally **k repeats** per task for variance. |
| **Safety** | No exfiltration bait; network via stubs or strict allowlists; redact logs/artifacts. |
| **Cost** | Per-task caps on rounds/tools/wall clock; **smoke** vs **nightly** tiers. |
| **Product alignment** | Tags that force staged paths, `patch_planner`, `acceptance`, `workflow_execute`, roles, `run_command` approvals. |
| **Anti-leakage** | Hidden checks separate from public prompts; accept public-benchmark memorization risk—mitigate with variants. |
| **Maintainability** | Stable `task_id`, `version`, `tags`, `min_crabmate_version`, `deprecated`. |
| **Fair A/B** | Change **one** knob (model or config) per experiment matrix; keep the rest fixed. |

---

## 3. Coverage matrix (suggested)

| Tier | Capability focus | Pass criteria |
|------|------------------|---------------|
| **L0** | Read-only tools, short answers | Substring / file exists |
| **L1** | Multi-step tools, small patches | `cargo check` or script exit code |
| **L2** | Plan–execute–verify | `steps[].acceptance` or external scoring (e.g. HumanEval path in **`docs/基准测试规划.md`** §5) |
| **L3** | Long runs, DAGs, recovery | Compose L1+L2 + timeout/retry stats |

Example **tags**: `readonly`, `patch`, `test_runner`, `workflow`, `staged_plan`, `http_stub`, `mcp`.

---

## 4. Recommended metadata (sidecar if not in JSONL)

| Field | Purpose |
|-------|---------|
| `task_id` | Stable key aligned with `benchmark_results.jsonl`. |
| `version` | Bump when prompt/fixture/scoring rules change. |
| `tags[]` | Filtering and dashboards. |
| `min_crabmate_version` | Skip on too-old binaries. |
| `deprecated` | Soft-remove from default CI. |
| `max_tool_rounds` / `timeout_secs` | Stricter caps than CLI defaults if needed. |
| `expected_artifacts[]` | Optional path globs for external validators. |

---

## 5. Evidence collection

| Source | Contains | Use |
|--------|----------|-----|
| **`benchmark_results.jsonl`** | Structured per-task outcome | Aggregate success/latency/failure phase |
| **Exported session JSON** (`save-session` / Web) | Full transcript | Mine new tasks; diff regressions |
| **`tool-replay`** | Deterministic tool replay | Decouple tool reliability from the LLM |
| **Logs** (`RUST_LOG`, `--log`) | Textual trace | Triage and coarse cost signals |
| **`GET /status`** | Queue, pipeline counters, `per_active_jobs` | Runtime mirrors; **final `plan_rewrite`** vs **staged patch** counters (see **`docs/en/DEVELOPMENT.md`**) |
| **Optional `thinking_trace`** | Phase/thinking events | Deep tuning (watch size & redaction) |

Suggested aggregates: group by `model` × `tags` × key config (e.g. `staged_plan_feedback_mode`)—success rate, mean rounds, rewrite exhaustion rate, patch-budget exhaustion rate, top `error_code`s.

---

## 6. CI tiers

| Tier | Content | Frequency |
|------|---------|-----------|
| **Contract** | JSONL parse, `validate_task`, tiny fixtures | **pre-commit / PR** |
| **Smoke bench** | Minimal `generic` or single-line tasks; prefer **no-LLM** paths when available | **daily** or gated PR |
| **Live LLM subset** | Secrets in CI only, hard timeouts | **nightly**; **not** default PR gate |

Never commit real API keys; truncate bodies in published reports.

---

## 7. System map

```text
Task JSONL / fixture workspace
        │
        ├─► crabmate bench  ──► benchmark_results.jsonl ──► dashboards
        │
        ├─► tool-replay (no LLM)
        │
        └─► session export ──► curate new tasks / regression cases
```

---

## 8. Governance

- **New task**: review for measurability, pinned env, safety, repeatability.
- **Deprecation**: keep `deprecated` tasks for **one release cycle** (or ~90 days) before deletion to preserve trend lines.
- **Scoring change**: bump `version` and write it into result rows so old and new scores are not mixed.

---

## 9. Revision history

| Date | Summary |
|------|---------|
| 2026-05-01 | Initial design note: dimensions, metadata, evidence, CI tiers, links to bench and replay. |
