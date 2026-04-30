**Languages / 语言:** English (this page) · [中文](../形式化验证计划.md)

# Formal verification plan (design draft)

This document describes a practical formal-verification layer for CrabMate on top of existing unit/golden/E2E/pre-commit checks. The near-term focus is **SSE protocol, state transitions, and key invariants**, then expansion to other high-risk paths.

## 1. Background

Existing tests are strong, but several risks remain:

- Protocol evolution can cause semantic drift between backend classifier logic and frontend dispatch order.
- Some defects are combinatorial (ordering and state combinations), where example-based tests are incomplete.
- Critical constraints (for example `stop/handled/plain` precedence) should be executable invariants, not only human conventions.

## 2. Goals and non-goals

### 2.1 Goals

- Make SSE control-plane invariants explicit and executable.
- Increase path coverage with property-based tests.
- Add model-level checks for key state machines (offline first, optional CI later).
- Keep code/tests/docs aligned as one contract.

### 2.2 Non-goals

- No one-shot migration to full theorem proving.
- No first-phase requirement for every module to use model checking.
- No replacement of E2E/integration tests; this is an additional safety layer for high-risk logic.

## 3. Verification scope by layer

1. **L1 (already prioritized)**: SSE protocol and control classification (shared Rust protocol crate + frontend dispatch ordering).
2. **L2 (near term)**: stream termination/cancel/conflict semantics (`StreamEndReason` chain).
3. **L3 (mid term)**: approval and tool-execution orchestration state machine.

## 4. Method mix

Use layered techniques instead of one tool:

- **Property testing (`proptest`)** for input/order spaces.
- **Golden fixtures** for cross-end contract stability.
- **Cross-end ordering snapshot tests** for branch-order drift.
- **Model checking (TLA+/PlusCal)** for safety/liveness of state machines (offline first).

## 5. SSE baseline invariants

These invariants should remain true long-term:

- If `error != null` and `code` is non-blank, classification must be `stop`, with precedence over handled branches.
- If any of `tool_call.summary`, `arguments_preview`, `arguments` is present and non-empty, classification is `handled`.
- `stream_ended.reason` must stay within the controlled enum set and parse consistently across ends.
- For golden control samples, shared Rust classifier and frontend dispatch classifier must agree.
- Relative order of critical frontend control branches must not invert (snapshot guard).

## 6. Phased rollout

### M0 (completed)

- Added `proptest` coverage in `src/sse/protocol.rs`.
- Added property tests for `crabmate-sse-protocol/control_classify.rs`.
- Added frontend `sse_dispatch` branch-order snapshot checks.
- Added one-command regression script: `./scripts/check-sse-protocol.sh`.

### M1 (1-2 weeks)

- Add `StreamEndReason` chain invariants (queue finalization, line classification, frontend consumption consistency).
- Add a “required invariants” section in `docs/en/SSE_PROTOCOL.md`.

### M2 (2-4 weeks)

- Build TLA+ model for approval/execution orchestration states (`Pending/Approved/Denied/Running/Ended`).
- Verify at least:
  - **Safety**: no illegal transitions (for example, sensitive command execution without approval).
  - **Liveness**: approved tasks eventually reach an end state (success/failure/cancelled).

### M3 (continuous)

- Add model checks as optional CI (nightly/manual), then promote to blocking for high-risk changes when stable.
- Add review checklist rule: new protocol keys must include new invariant tests.

## 7. CI and quality gates

Default local/commit gates:

- `pre-commit run --all-files`
- `./scripts/check-sse-protocol.sh`

Optional staged enhancements:

- `cargo test golden_sse_control --workspace`
- TLA+ checks as non-blocking warnings first, then blocking.

## 8. Artifacts and locations

- Code tests: `src/sse/protocol.rs`, `crates/crabmate-sse-protocol/control_classify.rs`
- Golden fixtures: `fixtures/sse_control_golden.jsonl`
- Script: `scripts/check-sse-protocol.sh`
- Design doc: `docs/形式化验证计划.md` (Chinese source)
- Future model files: `specs/tla+/` (for example `sse_control.tla`)

## 9. Risks and mitigations

- **Risk: longer test runtime**
  - Mitigation: stratified sample counts (fast local vs full CI).
- **Risk: brittle snapshots**
  - Mitigation: snapshot only critical checkpoints, not full-file text.
- **Risk: model/implementation drift**
  - Mitigation: model review must reference concrete code paths and invariant IDs.

## 10. Definition of done

The rollout is considered effective when:

- SSE protocol regressions that break invariants fail deterministically in local or CI tests.
- Cross-end drift (shared Rust classifier vs frontend dispatch) is auto-detected.
- Docs/tests/scripts form a reproducible loop that contributors can run and extend independently.
