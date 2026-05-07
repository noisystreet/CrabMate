**Languages / 语言:** English (this page) · [中文](../规划执行验证架构.md)

# Structured Plan-Execute-Verify (P-E-V) loop: architecture and design

**Status**: design draft (**no committed implementation timeline**).  
**Audience**: maintainers, product designers, protocol designers.  
**Related docs**: `docs/工作流编排架构.md` (DAG/FSM boundary), `docs/en/DEVELOPMENT.md` (`agent_turn` / `per_coord` / `plan_artifact` / staged planning), `docs/en/TOOLS.md` (`workflow_execute` / `agent_reply_plan` contracts), `docs/en/CONFIGURATION.md` (`final_plan_*` / `staged_plan_*` / `reflection_*`).

---

## 1. Goal

Without suppressing model creativity, reduce hidden dependence on one-shot model output and make the loop bounded, observable, and testable:

| Capability | Meaning |
|---|---|
| Explicit subtask decomposition | Structured steps with stable `id`, optional DAG/tool-role binding |
| Execute | Staged execution with `executor_kind` tool narrowing; optional in-turn `workflow_execute` DAG |
| Verify | Deterministic/replayable checks on execution facts (exit code, structured output, file fingerprints), not only LLM self-judgment |
| Reflect / retry | Bounded retry with clear separation between “plan-shape failure” and “acceptance-not-met” |

This repo already has multi-turn `agent_turn`, final plan validation/rewrite, workflow reflection, and optional side LLM semantic consistency checks. This document defines **gaps and recommended increments** without silently forking existing semantics.

---

## 2. Current implemented capabilities

### 2.1 Plan

- `agent_reply_plan` v1 (`src/agent/plan_artifact.rs`) with `type/version/steps[]`, step `id/description`, optional `workflow_node_id`, optional `executor_kind`.
- `workflow_validate_only` for “validate DAG first, then ask model for aligned plan”, including validate-only binding checks.

### 2.2 Execute

- Staged planning path (`agent_turn::staged`): tool-free planning round, inject steps as user messages, then full tool sub-loop per step with `sub_agent_policy` restrictions by `executor_kind`.
- `workflow_execute` DAG is orthogonal: in-turn dependency/parallel chain aligned via `workflow_node_id` when needed.

### 2.3 Verify (already implemented baseline)

- Deterministic step-level verifier (`step_verifier.rs`) checks current step’s relevant final tool evidence against `steps[].acceptance`.
- Current acceptance rule set includes:
  - `expect_exit_code`
  - `expect_stdout_contains`
  - `expect_stderr_contains`
  - `expect_file_exists`
  - `expect_json_path_equals` (legacy `$.a.b`, `$[0].k`, `$.items[0][1]`; JSON Pointer `/a/0`; empty path = whole JSON)
  - `expect_http_status` (for HTTP-like tools)
- Implicit verifier facts already exist in tool envelopes (`error_code`, exit code, structured fields).
- `final_plan_semantic_check_enabled` remains a plan-vs-summary consistency check, not a generic acceptance gate.
- **Audience / critic role (planned)**: a broader “structured commentary on plan / execute / reflect segments” design lives in **`docs/design/audience_critic_role.md`**. If implemented, it must align with **`plan_rewrite` / staged patch counters / `per_plan_semantic_check`** to avoid duplicate side calls and silent divergence.

### 2.4 Reflect / retry (partially implemented)

- `plan_rewrite_max_attempts` path for plan-format/binding failures.
- `WorkflowReflectionController` for whether/how to rerun workflow and inject reflection prompts.
- Staged feedback policy (`staged_plan_feedback_mode`, etc.).
- **Counters (orthogonal to `plan_rewrite`)**: `PerCoordinator` tracks `plan_rewrite_attempts` (final-answer `after_final_assistant`) vs `staged_plan_patch_planner_rounds_completed` (successful staged patch-planner merges). `GET /status` `per_active_jobs[*]` mirrors both plus `staged_plan_patch_max_attempts_config`; staged patch feedback user bodies and `StepRetryExhausted` messages append a `[计数]` footer for log correlation.

### 2.5 Hierarchical (`hierarchy`) vs PER / staged: dual-track contract

**Dual-track** means: end-of-turn **`per_coord::final_plan_gate`** + **`plan_rewrite`** + (optional) **`per_plan_semantic_check`** (semantic completion routed via **`run_final_plan_gate_semantic_completed`**), versus **Manager / Operator** decomposition, execution, subgoal verification, and **`reflect_and_replan`** under **`src/agent/hierarchy/`**, are **not the same code path or counters**, but share the same P-E-V mental model. **Convergence** means aligned **contracts and observability**, not forcing Manager through `AfterFinalAssistant`.

| Topic | PER / `outer_loop` / `staged` | `hierarchy` (hierarchical) | When changing code |
|------|-------------------------------|---------------------|-------------|
| **Final `agent_reply_plan` + `plan_rewrite`** | **`after_final_assistant`** → **`final_plan_gate`**; counter **`plan_rewrite_attempts`** | Manager emits **`SubGoal`** JSON, **not** final-plan gated; discourse/clarify/confirm falls back to **`run_agent_outer_loop`** then PER applies | Do not call **`ManagerAgent::reflect_and_replan`** “`plan_rewrite`”; final-answer rule edits stay in **`per_coord/final_plan_gate`** and **`agent_turn/reflect`** |
| **Step / subgoal acceptance** | **`step_verifier`** (staged step boundary); shares kernel with **`GoalAcceptance` / `verify_against_spec`** | **`GoalVerifier::run_verify_command`** etc.; failures drive **`reflect_and_replan`** | Changing **`acceptance`** semantics or tool-output checks: review **`step_verifier`** **and** **`hierarchy/goal_verifier`** (and reflection prompts) |
| **Workflow DAG reflection** | **`WorkflowReflectionController`**, `INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT` | DAG runs inside **`workflow_execute`**; Manager has its own reflection path | If adding prompt-like instructions parallel to reflection injects, document side-by-side in **`DEVELOPMENT.md`** and constants (see §6 checklist) |
| **Side semantic LLM (plan vs tool digest)** | **`per_plan_semantic_check`** + **`final_plan_gate::run_final_plan_gate_semantic_completed`** | **No** default equivalent | Do not treat hierarchical Manager reflection as **`final_plan_semantic_check`** |
| **Observability** | **`target: crabmate::agent_turn`**: `turn_orchestration_mode`; **`target: crabmate::per`**: gate routes | Same target: **`hierarchical_phase`** in **`agent_turn/hierarchy.rs`**; keep **`[HIERARCHICAL]`** `log` lines | Correlate **`turn_orchestration_mode`** with **`hierarchical_phase`**; maintenance checklist in **`docs/en/DEVELOPMENT.md`** “sync with `agent_turn` / `per_coord`” |

#### 2.5.1 Final answer & reflection: responsibility matrix (source anchors)

This complements the table above: **where the last assistant message comes from** and **what drives reflection vs rewrite**. Do not equate Manager **`reflect_and_replan`** with **`plan_rewrite`**.

| Capability | PER / `outer_loop` / `staged` | `hierarchy` main path | Intersection (discourse → `outer_loop`) |
|------------|-------------------------------|------------------------|----------------------------------------|
| **Shape of final assistant** | May require parseable **`agent_reply_plan` v1** (per **`final_plan_requirement`**); **`per_coord::after_final_assistant`** / **`final_plan_gate`** | **`handle_execution_result`** aggregates Markdown (**not** `agent_reply_plan` gating) | Same as PER track after entering **`run_agent_outer_loop`** |
| **`plan_rewrite` accounting** | **`PerCoordinator::plan_rewrite_attempts`** | **Not counted** (no **`after_final_assistant`**) | After fallback, **counts** on PER track (new **`PerCoordinator::new(PerCoordinatorInit::from_agent_config(...))`**) |
| **Reflection after step/subgoal failure** | Staged: **`patch_planner`** / **`fail_fast`**; outer loop: **`per_reflect_after_assistant`** context | **`ManagerAgent::reflect_and_replan`**; **not** `plan_rewrite` | No Manager; **`outer_loop`** R semantics |
| **Workflow DAG alignment** | **`WorkflowReflectionController`**, **`workflow_tool_dispatch`** | **`workflow_execute`** inside Operator; **`workflow_node_id`** rules in **`plan_artifact`** | Same as PER after fallback |
| **Side semantic LLM** | **`per_plan_semantic_check`** (optional) | **No** default equivalent | Applies when **`final_plan_semantic_check_enabled`** on PER path |

**Source anchors:** **`per_coord/final_plan_gate.rs`**, **`reflection/plan_rewrite.rs`**, **`per_coord/mod.rs`** (`AfterFinalAssistant`); **`agent_turn/hierarchy.rs`** (`DiscourseFallbackOuter` → **`run_agent_outer_loop`**); **`agent_turn/hierarchical_intent_route.rs`** (`resolve_hierarchical_post_intent_route`); **`agent_turn/intent/at_turn_start.rs`** (`run_intent_for_hierarchical`); **`agent_turn/hierarchy.rs`** (`handle_execution_result` / `emit_hierarchical_final_assistant`).

#### 2.5.2 Automated regression touchpoints (dual-track)

| Touchpoint | Command / test | Notes |
|------------|------------------|-------|
| Single-agent outer loop + tool + final (**PER `run_agent_outer_loop`**) | `cargo test -p crabmate run_agent_turn_outer_loop_tool_round_then_final_assistant` | `tests/run_agent_turn_orchestration_mock.rs` |
| Hierarchical Router→Manager→Operator (**no `PerCoordinator` final gate**) | `cargo test -p crabmate run_agent_turn_hierarchical_end_to_end_mock_llm_sequence` (and `run_hierarchical_router_manager_operator_mock_llm_sequence`) | Same file; pins **`run_agent_turn` → `run_hierarchical_agent` → `runner::run_hierarchical`** |
| **Hierarchical discourse → `outer_loop` (PER ∩ hierarchical)** | `cargo test -p crabmate run_agent_turn_hierarchical_discourse_fallback_uses_per_outer_loop` | User text **`你好`** → **`DiscourseFallbackOuter`** → **one** mock LLM (shared with **`PerCoordinator`** chain) |
| Fallback routing pure function | `cargo test -p crabmate hierarchical_intent_route` | Run when editing **`resolve_hierarchical_post_intent_route`** |

When changing **`final_plan_requirement` / `plan_rewrite` / `after_final_assistant` / `WorkflowReflectionController`**: run at least the **PER** touchpoints; if the intent pipeline changes, also **`cargo test -p crabmate golden_intent_regression`**. When changing **Manager reflection / subgoal acceptance**: run **hierarchical** touchpoints. When changing **discourse fallback or intent thresholds**: run the **intersection** touchpoint + **`hierarchical_intent_route`**.

---

## 3. Gaps and principles

### 3.1 Gaps

1. Acceptance is often implicit in natural-language step descriptions; deterministic replay suffers.
2. `plan_rewrite` (plan form/binding) and execution failure (acceptance unmet) are not unified as first-class, explicitly separated channels.
3. Outer-loop boundedness should align with the same philosophy as existing rewrite/reflection limits, with explicit reason codes.

### 3.2 Principles

| Principle | Description |
|---|---|
| Single structured plan source | Keep `plan_artifact` as the source of truth; backward-compatible serde evolution |
| Deterministic-first verification | Prefer tool facts over LLM judgment whenever possible |
| Orthogonal to `plan_rewrite` | Plan shape/binding errors vs world-state acceptance failures must remain separate |
| Bounded loops | Keep retry/replan/wall-clock/token caps explicit and conservative |
| Observable | Verification outcomes must enter messages/trace, not only opaque stderr |

---

## 4. Recommended architecture: three layers + gate

Core architecture intent:

- **Plan layer**: `agent_reply_plan` (+ optional `workflow_validate_only` pre-alignment)
- **Execute layer**: staged E as primary, optional in-turn DAG execution
- **Verify gate**: deterministic verifier yields `Pass | Fail(reason) | EscalateHuman`
- **Reflect/replan**: bounded local retry or structured replan prompt; `plan_rewrite` remains separate for shape/binding issues

---

## 5. PlanStep v1 extension direction (design-level optional fields)

Proposed optional fields (subject to schema/contract review):

| Field (proposal) | Type | Purpose |
|---|---|---|
| `step_kind` | enum | `implement` / `verify` / `gate` for UI and verifier behavior hints |
| `acceptance` | object | bounded deterministic acceptance spec |
| `max_step_retries` | integer | per-step local retry cap |

All optional; omitted fields keep current behavior.

---

## 6. Integration checklist with existing modules

| Module | Integration notes |
|---|---|
| `plan_artifact` | parse/validate extension fields; preserve current error-handling compatibility |
| `agent_turn::staged` | invoke verifier after each step E when enabled; branch to next step / local retry / stop |
| `per_coord` | keep final `plan_rewrite` semantics; use separate counters for verify-driven replans |
| `workflow_reflection_controller` | keep DAG reflection semantics; if verify failure needs injected prompts, define separate semantics clearly |
| `final_plan_semantic_check` | keep as plan-summary consistency, not deterministic acceptance replacement |
| SSE | any new control events/reason codes must be synchronized across `docs/en/SSE_PROTOCOL.md`, shared protocol crate, and frontend |

---

## 7. Non-goals and risks

- Do not introduce unrestricted expression-language evaluation for acceptance in first phase.
- Do not mutate `workflow_execute` DAG into a generic unbounded verifier machine.
- Do not allow unbounded “retry until model is satisfied” loops by default.

---

## 8. Suggested phases

| Phase | Status | Scope | Output |
|---|---|---|---|
| P0 | done | document recommended acceptance patterns | docs updates |
| P1 | done | verifier supports exit code + JSON path + HTTP status | gate + unit tests |
| P2 | in progress | optional `step_kind` / `acceptance` / `max_step_retries` in plan schema | serde migration + UI support if needed |
| P3 | pending | trace/status alignment for verify events | observability completeness |

---

## 9. Source index

| Topic | Path |
|---|---|
| Plan JSON | `src/agent/plan_artifact.rs` |
| Staged execution | `src/agent/agent_turn/staged/mod.rs` |
| Step verifier | `src/agent/step_verifier.rs` |
| PER / plan rewrite | `src/agent/per_coord/`, `src/agent/reflection/plan_rewrite.rs` |
| Workflow reflection | `src/agent/workflow_reflection_controller.rs` |
| Side semantic check | `src/agent/per_plan_semantic_check.rs` |
| Hierarchical (Manager/Operator) | `src/agent/hierarchy/`; entry `agent_turn/hierarchy.rs` (`hierarchical_phase` logs) |
| DAG execution | `src/agent/workflow/`, `src/agent/workflow_tool_dispatch.rs` |
| Validate-only | `src/agent/workflow/run.rs` |

---

## 10. Revision history

| Date | Summary |
|---|---|
| 2026-04-12 | Initial Chinese draft: P-E-V layering, mapping to existing capabilities, verifier-vs-plan-rewrite separation, staged roadmap and non-goals |
| 2026-04-16 | P1 completed in Chinese design: verifier supports JSON path and HTTP status acceptance checks |
| 2026-05-01 | Split staged patch-planner vs final `plan_rewrite` counters; extend `/status` `per_active_jobs` and patch feedback footers |
| 2026-05-02 | Added **§2.5.1–§2.5.2**: final-answer/reflection matrix and regression test commands; dual-track touchpoints aligned with Chinese §2.5. |
| 2026-05-01 | Added **§2.5**: hierarchical vs PER/staged dual-track responsibility boundary. |
