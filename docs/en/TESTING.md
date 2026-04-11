**Languages / 语言:** English (this page) · [中文](../TESTING.md)

# Testing and quality checks

This page lists **automated tests and common checks** for the CrabMate repo (run from the repository root unless noted). For module layout and protocols, see [`DEVELOPMENT.md`](DEVELOPMENT.md).

## Prerequisites

- **Rust**: 1.85+ (edition 2024); see [`README-en.md`](../../README-en.md).
- **E2E**: Node.js and npm; install Playwright’s Chromium once.
- **Web assets**: E2E and `serve` need **`frontend-leptos/dist/index.html`** — build with **`cd frontend-leptos && trunk build`** (use **`trunk build --release`** for production-sized WASM).

## Pre-commit

Aligned with [`.pre-commit-config.yaml`](../../.pre-commit-config.yaml):

```bash
pre-commit run --all-files
```

Includes (non-exhaustive):

- **`cargo fmt --all`**
- **`cargo clippy --all-targets --all-features -- -D warnings`**
- **`cargo test golden_sse_control`** (conditional hook when `fixtures/sse_control_golden.jsonl` or `src/sse/control_dispatch_mirror.rs` change)

Without pre-commit installed, run at least:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

Note: `pre-commit run --all-files` does **not** run `commit-msg`; message format is checked on **`git commit`** (see [`.cursor/rules/conventional-commits.mdc`](../../.cursor/rules/conventional-commits.mdc)).

## Rust: workspace tests

From the **repo root** (workspace members: `crabmate`, `crabmate-web-leptos`, `crabmate-sse-protocol`):

```bash
cargo test
```

### By package

| Package | Command | Notes |
| --- | --- | --- |
| Main binary + backend | `cargo test -p crabmate` | Most `src/` and `tests/` tests |
| SSE protocol crate | `cargo test -p crabmate-sse-protocol` | Version / doc marker self-checks, etc. |
| Web UI crate | `cargo test -p crabmate-web-leptos` | See **Frontend (Leptos)** below |

### Filter by test name (examples)

```bash
cargo test golden_sse_control
cargo test control_dispatch_mirror
cargo test tool_result_envelope_golden
```

If you change SSE **control-plane** branch ordering, update the golden fixture and run `golden_sse_control` (see [`SSE_PROTOCOL.md`](../SSE_PROTOCOL.md)). For cross-crate or public API changes before merge/release, prefer full **`cargo test`** (see [`.cursor/rules/rust-clippy-and-tests.mdc`](../../.cursor/rules/rust-clippy-and-tests.mdc)).

### Optional: nightly

```bash
cargo +nightly test
```

## Frontend (Leptos / `frontend-leptos`)

### Host target unit tests (default)

From repo root:

```bash
cargo test -p crabmate-web-leptos
```

Or:

```bash
cd frontend-leptos && cargo test
```

Covers Markdown sanitization, session helpers, `debounce_schedule`, etc. (no browser).

### WASM target tests (optional)

`wasm-bindgen-test` needs **`wasm-bindgen-cli`** matching the **`wasm-bindgen`** version in **`Cargo.lock`**, plus the wasm32 test runner. The lockfile currently pins **0.2.114**:

```bash
cargo install wasm-bindgen-cli --version 0.2.114 --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown -p crabmate-web-leptos
```

If `wasm-bindgen` is bumped in the lockfile, use that version in the install command.

### Typecheck only (no tests)

After protocol or large UI changes, at least:

```bash
cd frontend-leptos && cargo check --target wasm32-unknown-unknown
```

### Static bundle build (required for E2E / `serve`)

```bash
cd frontend-leptos && trunk build
# Production-sized WASM:
# cd frontend-leptos && trunk build --release
```

## Browser E2E (Playwright)

Directory: **`e2e/`**. Stubs **`POST /chat/stream`** and **`/workspace`** routes — **no real LLM**. Tests live under **`e2e/tests/`** (e.g. **`smoke.spec.ts`**); prefer **`data-testid`** selectors.

```bash
cd frontend-leptos && trunk build
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

- **Visual / layout smoke list**: [`docs/frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md`](../frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md) (no screenshot diff pipeline in-repo).

## See also

- Architecture and E2E detail: [`DEVELOPMENT.md`](DEVELOPMENT.md) (§ `frontend-leptos`, E2E)
- SSE contract and goldens: [`SSE_PROTOCOL.md`](../SSE_PROTOCOL.md)
- Debugging: [`DEBUG.md`](../DEBUG.md)
