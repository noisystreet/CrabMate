**Languages / 语言:** English (this page) · [中文](../测试指南.md)

# Testing and quality checks

This page lists **automated tests and common checks** for the CrabMate repo (run from the repository root unless noted). For module layout and protocols, see [`DEVELOPMENT.md`](DEVELOPMENT.md). For **`crabmate bench`** roadmap and benchmark-specific testing strategy, see [`基准测试规划.md`](../基准测试规划.md) (kept separate from this general checklist). For **how to design and operate task suites** (coverage, reproducibility, cost, CI tiers), see [`BENCHMARK_TASK_SUITE_DESIGN.md`](BENCHMARK_TASK_SUITE_DESIGN.md). **HumanEval scoring script smoke** (no LLM): `python3 scripts/humaneval_score_benchmark_results.py --tasks fixtures/benchmark/humaneval_tiny_tasks.jsonl --results fixtures/benchmark/humaneval_tiny_results.jsonl --output /tmp/tiny_scores.jsonl` (executes the tiny completion; see `基准测试规划.md` §5.3).

## Prerequisites

- **Rust**: 1.85+ (edition 2024); see [`README.md`](../../README.md).
- **E2E**: **Playwright** and **Victauri** live in [crabmate-client](https://github.com/noisystreet/crabmate-client) (local sibling `../crabmate-client`; `./scripts/e2e-playwright.sh` honors **`CRABMATE_CLIENT_DIR`**). This repo keeps `crabmate e2e` / HTTP real-LLM tests.
- **Web assets**: E2E and `serve --with-web` need **`frontend/dist/index.html`** — build in the Client repo with **`make frontend`**, then set **`CM_WEB_STATIC_DIR`**. API-only: default **`serve`**.

## GitHub Actions (main CI)

Push / pull request to **`main`** runs [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml):

- **`workspace`** (check · clippy · test): **`cargo check`**, **`cargo clippy`** (**`-D warnings`**), **`cargo test --workspace --all-features`** (this repo has **no** Tauri/GTK desktop job; shell CI lives in the Client repo)
- **`client-contract`**: contract / SSE golden / OpenAPI gate (**`scripts/check-client-contract.sh`**)
- **`build-release`** (package): **`make package`** (server-only **`tar.gz` + `.deb`**, no frontend; needs **`cargo-deb`**)

Complexity, dependency security, and coverage use separate workflows (**`code-complexity.yml`**, **`dependency-security.yml`**, **`code-coverage.yml`**).

### GitHub Release (tags only)

[`.github/workflows/release.yml`](../../.github/workflows/release.yml) does **not** run on PR/`main` pushes. It runs when you push tag **`vX.Y.Z`** (or **`workflow_dispatch`** with an existing tag): **`make package`**, create/update a GitHub Release, attach artifacts. Body comes from **`CHANGELOG.md`** for the tag’s core **`X.Y.Z`** (so **`v0.1.0-rc.1`** still uses the **`[0.1.0]`** section while **`Cargo.toml`** may stay **`0.1.0`**). Re-running for the same tag updates that Release.

## Pre-commit

Aligned with [`.pre-commit-config.yaml`](../../.pre-commit-config.yaml):

```bash
pre-commit run --all-files
```

Includes (non-exhaustive):

- **`cargo fmt --all`**
- **`cargo clippy --all-targets --all-features -- -D warnings`**
- **`lizard-rust`**: Rust cyclomatic complexity (requires **`pip install lizard`**; **`scripts/lizard-rust.sh`** / **`scripts/lizard_rust_metrics.py`**: every function under **`src/`** must have **CCN ≤ 10**). Optional **`--list-above N`** lists functions above that warning threshold
- **`fn-param-ratchet`**: Rust function parameter counts (**`scripts/fn-param-ratchet.sh`** / **`scripts/fn_param_rust_metrics.py`**; hard cap **32** and `scripts/fn_param_*.txt` baselines are fixed in Python)
- **`fn-nloc-ratchet`**: Rust function-body **`nloc`** (lizard) plus **physical `.rs` file line counts** (same script **`scripts/fn-nloc-ratchet.sh`** / **`scripts/fn_nloc_rust_metrics.py`**; baseline paths and write-back policy are fixed in Python); function ratchets **`scripts/fn_nloc_max_baseline.txt`**, **`scripts/fn_nloc_top10_sum_baseline.txt`**; file ratchets **`scripts/rust_file_max_lines_baseline.txt`**, **`scripts/rust_file_top10_lines_sum_baseline.txt`**; runs in **`.github/workflows/code-complexity.yml`**
- **Coverage**: **`.github/workflows/code-coverage.yml`** is **manual-only** (`workflow_dispatch`); locally you can still run `cargo llvm-cov` + **`scripts/check_coverage_ratchet.py`**
- **`./scripts/check-sse-protocol.sh`** (when changing SSE / `fixtures/sse_ag_ui_golden.jsonl` / **`fixtures/http_sse_failure_path_golden.jsonl`**)

Without pre-commit installed, run at least:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
bash scripts/lizard-rust.sh
bash scripts/fn-param-ratchet.sh
bash scripts/fn-nloc-ratchet.sh
```

Note: `pre-commit run --all-files` does **not** run `commit-msg`; message format is checked on **`git commit`** (see [`.cursor/rules/conventional-commits.mdc`](../../.cursor/rules/conventional-commits.mdc)).

## Rust: unit and integration tests

From the **repo root** (single crate **`crabmate`**; **not** Client `crabmate-web`):

```bash
cargo test
```

### By scope

| Scope | Command | Notes |
| --- | --- | --- |
| Main binary + backend | `cargo test -p crabmate` | Most `src/` and `tests/` tests |
| Wire contract (no tokio) | `cargo test --lib --no-default-features --features protocol cm_sse_protocol` | `cm_sse_protocol` classify/frames; also **`./scripts/check-sse-protocol.sh`** |
| OpenAPI / HTTP shell | `cargo test --lib openapi_` | `GET /openapi.json` vs axum `.route(`; Leptos UI tests are in Client |
| Light HTTP smoke (no LLM) | `cargo test -p crabmate --lib workspace_file_raw_http_smoke` | `src/test_serve.rs` random port; see below |

### Lightweight HTTP integration tests (`start_test_serve`)

Prefer **`#[tokio::test]` next to handlers** (`cargo test --lib`) for route + workspace security + status codes. Do not default to `tests/` real-LLM e2e.

1. **`start_test_serve(None)`**: random port, default config, `build_tools()`, **no Bearer middleware**, no LLM.
2. **`tempfile` workspace** + `POST /workspace` `{"path": …}` to set the root. Empty `workspace_allowed_roots` allows any path except sensitive prefixes (`/etc`, `/root`, …).
3. **`reqwest::Client::builder().no_proxy()`**: otherwise `HTTP_PROXY` (e.g. Privoxy) hijacks loopback. Same idea as `no_proxy=127.0.0.1,localhost` in e2e.
4. **One serve, several asserts** (e.g. GET raw 200 / 415 / 400, PUT 204 / 409) so you do not pay `load_config` per case.
5. **lizard** counts test functions in `src/`; keep CCN ≤ 10 and file-line ratchets; split HTTP tests into `*_http_tests.rs` if needed.

Example: `src/web/workspace/handlers_file_raw_http_tests.rs`.

### Filter by test name (examples)

```bash
./scripts/check-sse-protocol.sh
cargo test tool_result_envelope_golden
```

If you change AG-UI control-plane dispatch, update **`fixtures/sse_ag_ui_golden.jsonl`** and run the frontend golden test (see [`SSE_PROTOCOL.md`](SSE_PROTOCOL.md)). For cross-crate or public API changes before merge/release, prefer full **`cargo test`** (see [`.cursor/rules/rust-clippy-and-tests.mdc`](../../.cursor/rules/rust-clippy-and-tests.mdc)).

### Optional: nightly

```bash
cargo +nightly test
```

## Frontend (Leptos / Client `frontend`)

Business UI lives in [crabmate-client](https://github.com/noisystreet/crabmate-client). Run the following from sibling **`../crabmate-client`** (or set **`CRABMATE_CLIENT_DIR`**).

### Host target unit tests (default)

```bash
cd ../crabmate-client/frontend && cargo test
```

Or:

```bash
cd ../crabmate-client && cargo test -p crabmate-web
```

Covers Markdown sanitization, session helpers, `debounce_schedule`, etc. (no browser).

### WASM target tests (optional)

`wasm-bindgen-test` needs **`wasm-bindgen-cli`** matching **`wasm-bindgen`** in the **Client** `frontend/Cargo.lock`, plus the wasm32 test runner. Example (pin to the lockfile version):

```bash
cargo install wasm-bindgen-cli --version 0.2.114 --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown -p crabmate-web
```

If `wasm-bindgen` is bumped in the lockfile, use that version in the install command.

### Typecheck only (no tests)

After protocol or large UI changes, at least:

```bash
cd ../crabmate-client && cargo check -p crabmate-web --target wasm32-unknown-unknown
```

### Static bundle build (required for E2E / `serve`)

```bash
cd ../crabmate-client && make frontend
# Production-sized WASM:
# cd ../crabmate-client && make frontend --release
```

## Desktop E2E (Victauri)

> **Canonical entry is only in the Client repo** [`crabmate-client`](https://github.com/noisystreet/crabmate-client) (local sibling `../crabmate-client`; see [`docs/TESTING.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/TESTING.md) there). This repo no longer ships `desktop-tauri/` or `scripts/victauri-e2e.sh`.

Directory: Client [`desktop-tauri/src-tauri/tests/`](https://github.com/noisystreet/crabmate-client/tree/main/desktop-tauri/src-tauri/tests) (local sibling `../crabmate-client/desktop-tauri/src-tauri/tests/`). Runs inside the **Tauri WebView** via [Victauri](https://github.com/runyourempire/victauri) (`victauri-test`). Seeds data with in-webview **`fetch()`** against `/user-data/*` and **`CM_E2E_FIXTURES=1`** backend routes; stubs **`POST /chat/stream`** with **`eval_js`** fetch interceptors where needed — **no real LLM** (except the opt-in **`victauri_real_llm`** suite). Prefer **`data-testid`**. See also [`docs/测试指南.md`](../测试指南.md) § 桌面端到端.

| Phase | Examples | Notes |
| --- | --- | --- |
| 1 | `victauri_session_crud`, `victauri_settings`, `victauri_prefs_theme` | UI + prefs |
| 2 | `victauri_keyboard`, `victauri_pagination`, `victauri_conversation` | API seed, no stream stub |
| 3 | `victauri_sse_stub`, `victauri_turn_layout`, `victauri_scroll_send` | SSE / workspace fetch stubs |
| 4 | `victauri_real_llm` | **`REAL_LLM_E2E=1`**, manual only |

### Local run

**One-shot (recommended)** — from the Client repo root (**`exec xvfb-run`** relaunches the script so the window never lands on your Wayland/X desktop; default **`VICTAURI_USE_XVFB=1`**):

```bash
cd ../crabmate-client
# optional: CM_DESKTOP_BACKEND_BIN=/path/to/crabmate
./scripts/victauri-e2e.sh victauri_scroll_send
./scripts/victauri-e2e.sh all
```

**Manual** (native display; start this repo's `serve` first):

```bash
# terminal A (this repo; transitional SPA hosting)
cd ../crabmate-client && make frontend
export CM_WEB_STATIC_DIR="$PWD/frontend/dist"
cd ../crabmate_agent
cargo run -- serve --with-web --host 127.0.0.1 --port 18080

# terminal B (Client repo)
cd ../crabmate-client/desktop-tauri/src-tauri
CM_E2E_FIXTURES=1 CM_DESKTOP_SERVE_URL=http://127.0.0.1:18080/ cargo tauri dev

# terminal C (same src-tauri)
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 cargo test --no-fail-fast
```

The one-shot script starts **`serve`** itself (default port **18080**) before the desktop shell.

### xvfb / headless (Linux)

| Variable | Default | Meaning |
| --- | --- | --- |
| **`VICTAURI_USE_XVFB`** | **`1`** | **`1`**: **`exec xvfb-run`** relaunch (no popup); **`0`**: native window; **`auto`**: xvfb when no **`DISPLAY`** or **`CI=true`** |
| **`VICTAURI_START_TIMEOUT`** | **`90`** | Seconds to wait for **`http://127.0.0.1:7373/health`** |
| **`VICTAURI_MAIN_WINDOW_WAIT`** | **`15`** | Extra settle time after health before tests |
| **`VICTAURI_PORT`** | **`7373`** | Victauri MCP port |

Install **`xvfb`** on Debian/Ubuntu: **`sudo apt install xvfb`**.

Force headless on a machine with **`DISPLAY`** (Client repo root):

```bash
cd ../crabmate-client
VICTAURI_USE_XVFB=1 ./scripts/victauri-e2e.sh victauri_scroll_send
```

Without **`VICTAURI_E2E=1`**, Victauri tests **skip**. Full suites: Client repo **`./scripts/victauri-e2e.sh all`** (not this repo's CI).

**Real-model E2E** (e.g. DeepSeek) is manual opt-in (**`REAL_LLM_E2E=1`**, not default CI). Full steps: [`docs/真实LLM-E2E.md`](../真实LLM-E2E.md) · summary [`REAL_LLM_E2E.md`](REAL_LLM_E2E.md).

Quick smoke (Client repo):

```bash
cd ../crabmate-client/desktop-tauri/src-tauri
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 REAL_LLM_E2E=1 API_KEY=YOUR_API_KEY \
  cargo test --test victauri_real_llm -- --nocapture
```

On Linux, if Tauri build fails on **wayland** native deps, see [`DEVELOPMENT.md`](DEVELOPMENT.md) (**`libwayland-dev`**).

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

- Architecture and E2E detail: [`DEVELOPMENT.md`](DEVELOPMENT.md) (§ `frontend`, E2E). UI source: Client [`frontend/`](https://github.com/noisystreet/crabmate-client/tree/main/frontend) (this repo’s [`frontend/ARCHITECTURE.md`](../frontend/ARCHITECTURE.md) is a pointer only). Playwright: Client [`e2e/`](https://github.com/noisystreet/crabmate-client/tree/main/e2e)
- SSE contract and goldens: [`SSE_PROTOCOL.md`](../SSE协议.md)
- Debugging: [`DEBUG.md`](../调试指南.md)
