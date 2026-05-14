**Languages / 语言:** English (this page) · [中文](../测试指南.md)

# Testing and quality checks

This page lists **automated tests and common checks** for the CrabMate repo (run from the repository root unless noted). For module layout and protocols, see [`DEVELOPMENT.md`](DEVELOPMENT.md). For **`crabmate bench`** roadmap and benchmark-specific testing strategy, see [`基准测试规划.md`](../基准测试规划.md) (kept separate from this general checklist). For **how to design and operate task suites** (coverage, reproducibility, cost, CI tiers), see [`BENCHMARK_TASK_SUITE_DESIGN.md`](BENCHMARK_TASK_SUITE_DESIGN.md). **HumanEval scoring script smoke** (no LLM): `python3 scripts/humaneval_score_benchmark_results.py --tasks fixtures/benchmark/humaneval_tiny_tasks.jsonl --results fixtures/benchmark/humaneval_tiny_results.jsonl --output /tmp/tiny_scores.jsonl` (executes the tiny completion; see `基准测试规划.md` §5.3).

## Prerequisites

- **Rust**: 1.85+ (edition 2024); see [`README-en.md`](../../README-en.md).
- **E2E**: Node.js and npm; install Playwright’s Chromium once.
- **Web assets**: E2E and `serve` need **`frontend/dist/index.html`** — build with **`cd frontend && trunk build`** (use **`trunk build --release`** for production-sized WASM).

## Pre-commit

Aligned with [`.pre-commit-config.yaml`](../../.pre-commit-config.yaml):

```bash
pre-commit run --all-files
```

Includes (non-exhaustive):

- **`cargo fmt --all`**
- **`cargo clippy --all-targets --all-features -- -D warnings`**
- **`lizard-rust`**: Rust cyclomatic complexity (requires **`pip install lizard`**; **`scripts/lizard-rust.sh`** / **`scripts/lizard_rust_metrics.py`**: per-function CCN hard cap **15**)
- **`fn-nloc-ratchet`**: Rust function-body **`nloc`** (lizard) plus **physical `.rs` file line counts** (same script **`scripts/fn-nloc-ratchet.sh`** / **`scripts/fn_nloc_rust_metrics.py`**); function ratchets **`scripts/fn_nloc_max_baseline.txt`**, **`scripts/fn_nloc_top10_sum_baseline.txt`**; optional hard cap **`FN_NLOC_CAP`**; file ratchets **`scripts/rust_file_max_lines_baseline.txt`**, **`scripts/rust_file_top10_lines_sum_baseline.txt`**; optional hard cap **`RUST_FILE_LINES_MAX_CAP`**; runs in **`.github/workflows/code-complexity.yml`**
- **Coverage**: **`.github/workflows/code-coverage.yml`** is **manual-only** (`workflow_dispatch`); locally you can still run `cargo llvm-cov` + **`scripts/check_coverage_ratchet.py`**
- **`cargo test golden_sse_control`** (conditional hook when `fixtures/sse_control_golden.jsonl`, `crates/crabmate-sse-protocol/control_classify.rs`, or any file under `frontend/src/sse_dispatch/` change)

Without pre-commit installed, run at least:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
bash scripts/lizard-rust.sh
bash scripts/fn-param-ratchet.sh
bash scripts/fn-nloc-ratchet.sh
```

Note: `pre-commit run --all-files` does **not** run `commit-msg`; message format is checked on **`git commit`** (see [`.cursor/rules/conventional-commits.mdc`](../../.cursor/rules/conventional-commits.mdc)).

## Rust: workspace tests

From the **repo root** (workspace members: `crabmate`, `crabmate-web`, `crabmate-sse-protocol`):

```bash
cargo test
```

### By package

| Package | Command | Notes |
| --- | --- | --- |
| Main binary + backend | `cargo test -p crabmate` | Most `src/` and `tests/` tests |
| SSE protocol crate | `cargo test -p crabmate-sse-protocol` | Version / doc marker self-checks, etc. |
| Web UI crate | `cargo test -p crabmate-web` | See **Frontend (Leptos)** below |

### Filter by test name (examples)

```bash
cargo test golden_sse_control
cargo test -p crabmate-sse-protocol golden_sse_control
cargo test tool_result_envelope_golden
```

If you change SSE **control-plane** branch ordering, update the golden fixture and run `golden_sse_control` (see [`SSE_PROTOCOL.md`](../SSE协议.md)). For cross-crate or public API changes before merge/release, prefer full **`cargo test`** (see [`.cursor/rules/rust-clippy-and-tests.mdc`](../../.cursor/rules/rust-clippy-and-tests.mdc)).

### Optional: nightly

```bash
cargo +nightly test
```

## Frontend (Leptos / `frontend`)

### Host target unit tests (default)

From repo root:

```bash
cargo test -p crabmate-web
```

Or:

```bash
cd frontend && cargo test
```

Covers Markdown sanitization, session helpers, `debounce_schedule`, etc. (no browser).

### WASM target tests (optional)

`wasm-bindgen-test` needs **`wasm-bindgen-cli`** matching the **`wasm-bindgen`** version in **`Cargo.lock`**, plus the wasm32 test runner. The lockfile currently pins **0.2.114**:

```bash
cargo install wasm-bindgen-cli --version 0.2.114 --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown -p crabmate-web
```

If `wasm-bindgen` is bumped in the lockfile, use that version in the install command.

### Typecheck only (no tests)

After protocol or large UI changes, at least:

```bash
cd frontend && cargo check --target wasm32-unknown-unknown
```

### Static bundle build (required for E2E / `serve`)

```bash
cd frontend && trunk build
# Production-sized WASM:
# cd frontend && trunk build --release
```

## Browser E2E (Playwright)

Directory: **`e2e/`**. Stubs **`POST /chat/stream`** and **`/workspace`** routes — **no real LLM**. Tests live under **`e2e/tests/`** (e.g. **`smoke.spec.ts`**); prefer **`data-testid`** selectors.

```bash
cd frontend && trunk build
cd ../e2e && npm ci
npx playwright install chromium
npm test
```

Notes:

- **`playwright.config.ts`** starts **`cargo run -- serve --port 18081`** and waits for **`GET /health`**.
- Override port with **`E2E_PORT`**, e.g. `E2E_PORT=19090 npm test`.
- Locally (non-CI), an existing server on that port may be **reused** (`reuseExistingServer`).
- Debug UI: `cd e2e && npm run test:ui`.

On Linux, if `cargo` fails on **wayland** native deps, see the E2E note in [`DEVELOPMENT.md`](DEVELOPMENT.md) (**`libwayland-dev`**).

## Dependency security and licenses (CI parity)

Workflow: [`.github/workflows/dependency-security.yml`](../../.github/workflows/dependency-security.yml). With **`cargo-audit`** and **`cargo-deny`** installed:

```bash
cargo audit
cargo deny check licenses bans sources
```

Policy file: root **`deny.toml`**. These checks are **not** in pre-commit to avoid fetching advisory DB on every commit.

## Not automated

- **Visual / layout smoke list**: [`docs/frontend/VISUAL_REGRESSION_CHECKLIST.md`](../frontend/VISUAL_REGRESSION_CHECKLIST.md) (no screenshot diff pipeline in-repo).

## See also

- Architecture and E2E detail: [`DEVELOPMENT.md`](DEVELOPMENT.md) (§ `frontend`, E2E)
- SSE contract and goldens: [`SSE_PROTOCOL.md`](../SSE协议.md)
- Debugging: [`DEBUG.md`](../调试指南.md)
