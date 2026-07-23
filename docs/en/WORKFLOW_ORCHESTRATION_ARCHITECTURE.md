**Languages / 语言:** English (this page) · [中文](../工作流编排架构.md)

# Workflow orchestration extensions: state machine, condition, and loop (architecture design)

**Status**: design draft (**no committed implementation timeline**).  
**Audience**: maintainers and product/protocol designers.  
**Related docs**: runtime behavior remains defined by `workflow_execute` / `workflow_validate` contracts in **`docs/en/TOOLS.md`** and architecture sections in **`docs/en/DEVELOPMENT.md`**. This document defines **capability boundaries** and **recommended evolution directions** to avoid silently diverging from today's DAG semantics.

---

## 1. Background

CrabMate already has:

- **In-turn DAG orchestration** via `workflow_execute` (`src/agent/workflow/`): topological deps, per-layer parallelism, `fail_fast`, `compensate_on_failure` / `compensate_with`, node-level `max_retries` (retryable errors only), `trace` / `workflow_run_id`, and optional Chrome Trace.
- **Session-level multi-step orchestration** in `agent_turn`: outer P/R/E loop, final `agent_reply_plan` v1, and workflow reflection (`workflow_reflection_controller`, `workflow_node_id` alignment with DAG nodes).

Roadmap and product expectations are moving toward state-machine-style configurations and readable conditional/loop expression. The core tension is that current `WorkflowSpec` is a **DAG**, not a native FSM/cyclic graph runtime.

If we add syntax without explicit boundaries, risks include scheduler complexity blow-up, approval/trace/plan-alignment inconsistency, and termination safety regressions.

---

## 2. Design goals

| Goal | Description |
|---|---|
| **Readable** | Operators/authors can see current business state and transition rationale |
| **Executable** | Single-process schedulable with timeout/wall-clock bounds, aligned with approval, `tool_call_explain`, and SSE |
| **Observable** | Reuse/extend `workflow_run_id`, `trace`, `completion_order`; new concepts must map to trace events |
| **Gradually compatible** | Preserve existing `workflow.nodes + deps` behavior by default; new power comes via opt-in fields/kinds/compile steps; no silent DAG behavior changes |

---

## 3. Current orchestration taxonomy

1. **Intra-turn orchestration**  
   `workflow_execute` triggered by tool calls, completed inside one execution step in a single turn.

2. **Inter-turn orchestration**
   Multi-turn P → E → P flows via outer loop and PER-related components.

3. **Declarative plan layer**  
   `agent_reply_plan` v1 with `id` / `workflow_node_id` / `executor_kind` alignment rules.

Key conclusion: if “state machine” means multi-turn business stages, it naturally belongs to (2)+(3), not as unrestricted cycles inside (1).

---

## 4. Target concepts and mapping strategy

### 4.1 State machine (FSM)

Recommended semantics: FSM is configuration-layer abstraction (“named states + guarded transitions”). Execution should map to one of:

- **A. Compile to DAG (preferred MVP)**  
  Config-level `states`/`transitions` expands into `WorkflowNodeSpec + deps` (with optional barrier/choice patterns).
  - Pros: reuse current scheduling, compensation, approval, trace, and schema-check chain.
  - Trade-off: runtime-dynamic branch targets require explicit guard/choice compile patterns.

- **B. Native FSM executor (long term)**  
  Parallel execution kind (for example `WorkflowKind::Fsm`).
  - Pros: natural dynamic transition modeling.
  - Trade-off: dual semantics + larger test matrix + result-shape compatibility burden.

Documentation rule: whenever “state machine” is mentioned externally, explicitly label whether it is “FSM compiled to DAG” or “native FSM engine”.

### 4.2 Conditions (branching)

| Layer | Mechanism | Readability strategy |
|---|---|---|
| Inside DAG | readonly check nodes + `deps` sequencing | naming convention (`check_*` → `act_*`) and optional display metadata |
| Explicit DAG branching (future) | choice nodes or result-based edges | scheduler writes skipped-branch trace events |
| Across turns | model emits next workflow/plan | handled by session-level narration and control |

Guard expressions should avoid free-form expression languages initially. Prefer “tool-as-guard”: controlled readonly tools emit structured JSON (for example `branch: "a" | "b"`), choice logic parses fixed schema.

### 4.3 Loops

Principle: no unbounded cycles in single-turn DAG execution.

Safe subsets:

1. **Bounded expansion** (`for_each`, `repeat N`) with hard limits (`max_items`, `max_iterations`)
2. **Outer-loop retries** at agent/session level (each DAG still acyclic)
3. **No default `while(true)` support**; any future general loop must include strict step/wall-clock/token caps and iteration-level trace visibility

---

## 5. Relation with workflow reflection

- **`workflow_execute` DAG** fits intra-turn parallel dependency chains.
- **Workflow reflection + `workflow_node_id`** should remain valid even if FSM is compiled into physical DAG nodes; mapping rules must stay explicit when logical and physical node IDs differ.

Recommended product narrative:

- "in-turn tool pipeline" → DAG (or FSM→DAG)
- "long-horizon decomposition" → `agent_reply_plan` + multi-turn execution

---

## 6. Observability and contract checklist

Any extension should explicitly define:

1. Result shape compatibility (`workflow_execute_result` or sibling result) with at least `workflow_run_id + status + trace`
2. Approval semantics for dynamic branches (lazy approval vs branch-scoped approval keys)
3. Compatibility with Chrome Trace / request-trace merge behavior
4. Workspace change semantics (whether new executors inherit current `ToolContext` behavior)

---

## 7. Suggested roadmap

| Phase | Scope | Output |
|---|---|---|
| Phase 0 | docs/examples: DAG naming conventions, condition-chain templates, outer-loop examples | updates in `docs/en/TOOLS.md` + this design doc |
| Phase 1 | config-level FSM compiled into `WorkflowSpec` (no fully dynamic runtime transitions) | compile module and unit tests for expansion/limits |
| Phase 2 | choice-node support + trace branch-pruning semantics | parser/scheduler updates + golden fixtures |
| Phase 3 | dynamic guards + bounded loops + approval policy integration | design + security reviews |

Out of scope for current consensus: making `workflow_execute` simultaneously support unrestricted Turing-complete scripting and unbounded loops without additional resource caps.

---

## 8. Source index (implementation entry points)

| Area | Path | Notes |
|---|---|---|
| DAG model | `src/agent/workflow/model.rs` | `WorkflowSpec` / `WorkflowNodeSpec` |
| Parsing | `src/agent/workflow/parse.rs` | `parse_workflow_spec` |
| Topology | `src/agent/workflow/dag.rs` | `topo_layers` |
| Scheduling/execution | `src/agent/workflow/execute/` | schedule / retry / compensation |
| Tool dispatch | `src/agent/workflow_tool_dispatch.rs` | `dispatch_workflow_execute_tool` |
| Required-field checks | `src/tools/schema_check.rs` | `workflow_tool_args_satisfy_required` |
| Plan alignment | `src/agent/plan_artifact.rs` etc. | `workflow_node_id` and validate-only binding |

---

## 9. Revision history

| Date | Summary |
|---|---|
| 2026-04-12 | Initial draft in Chinese: layered model (intra-turn DAG / inter-turn orchestration / declarative plan), FSM-compile-first direction, condition/loop boundaries. |
