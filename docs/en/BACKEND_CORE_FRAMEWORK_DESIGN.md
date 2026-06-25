**Languages / 语言:** English (this page) · [中文](../后端核心框架设计.md)

# Backend core frameworkization and cross-language embedding: current state and gaps

This document captures a **design analysis** (not an implementation commitment) for evolving CrabMate’s backend core into a reusable framework, and for enabling future hosting from **Python or other languages**. For current module responsibilities and layering, see **`docs/en/DEVELOPMENT.md`** (architecture overview).

## 1. Background and goals

- **Goal**: decouple core logic (agent turns, LLM calls, tool orchestration, config/types) from product shells (built-in Web/CLI/TUI and full dependency set), so it can:
  - be consumed as a **Rust library** by other projects;
  - be called by **Python/other runtimes** via FFI, subprocess RPC, or HTTP.
- **Non-goals in this doc**: final choice of PyO3/maturin, concrete gRPC schemas, or final crate naming.

## 2. Current structure (brief)

- The root **`crabmate`** crate still hosts agent/runtime logic plus **Axum Web**, **CLI** (`clap`), **TUI** (`crossterm`/`reedline`), optional memory embeddings (`fastembed`), MCP, and Docker sandbox (`bollard`).
- Workspace-level extraction already exists for:
  - `frontend` (Web UI)
  - `crates/crabmate-sse-protocol` (SSE control-plane contract)
  - `crates/crabmate-types` (OpenAI-compatible messages; re-exported as `types`)
  - `crates/crabmate-config` (configuration loading and `AgentConfig`; re-exported as `config`)
  - `crates/crabmate-llm` (vendor adapters, HTTP client, LLM errors/backend trait; `llm` module re-exports)
- Public API is still application-shaped: some `pub use` exports and `run_agent_turn` entrypoints exist, but there is no clearly versioned “framework surface”.

## 3. Gap analysis

### 3.1 Crate boundaries and dependency graph

| Observation | Impact |
|---|---|
| Single root crate aggregates Web/CLI/TUI and heavy deps | Embedders cannot easily link “core-only”; build size/time are harder to trim |
| Missing capability-focused boundaries (crate split or deeper feature partition) | Product mode and library mode remain coupled |

**Already landed (dependency trimming):**
- Root features: **`mcp`**, **`docker_sandbox`**, **`fastembed`**
- Default remains full product: `default = ["mcp", "docker_sandbox", "fastembed"]`
- Example trims: `cargo build --no-default-features` or a selected subset
- Without `fastembed`, config finalize coerces vector backend to disabled and semantic search falls back appropriately

**Conclusion**: for a real framework surface, Web/TUI still need to be optionalized (by crate split and/or additional features).

### 3.2 External API shape

| Observation | Impact |
|---|---|
| `RunAgentTurnParams` carries many concerns (streaming, runtime hooks, tracing, memory scopes) | Hard to offer a narrow stable interface for non-Rust hosts |
| No dedicated semver stability policy for an API subset | Refactors can accidentally break consumers |

**Conclusion**: converge to a narrow engine/session-style interface with typed errors and explicit injection points (approval/log/cancel/tool runtime), instead of growing a mega-params struct.

### 3.3 Python / cross-language integration

| Path | Current state and gap |
|---|---|
| Out-of-process (HTTP + SSE) | Production-ready path exists; SSE contract is shared. Gap: no dedicated non-Web RPC contract with explicit stability promises |
| In-process (FFI/PyO3) | No stable C ABI/PyO3 layer yet; async runtime and GIL/asyncio bridge strategy must be defined |
| Sync host runtime | Core path is async; blocking bridge requires explicit runtime ownership decisions |

**Conclusion**: choose the boundary first (subprocess RPC vs embedded FFI), then derive crate/API partitioning accordingly.

### 3.4 Runtime and global state

| Observation | Impact |
|---|---|
| Logging/config/workspace/session concerns are intertwined with CLI/Web flows | Library users need explicit context injection instead of implicit global boot order |
| Tool sandbox/allowlist/approval are strongly tied to current registry/runtime wiring | Third-party hosts need documented extension points to avoid forking internals |

### 3.5 Evolution cost

- Splitting `src/` into multiple crates has short-term migration cost (visibility/import churn), but improves long-term versioning and CI matrix granularity.
- A stable framework API should ship with explicit stability guarantees and minimal embedding-focused integration tests.

## 4. Suggested evolution order

1. **Dependency layering**: make no-Axum/no-TUI build paths first-class.
2. **Stable surface**: narrow exported types/functions; document error/cancel semantics.
3. **Cross-language boundary**: choose subprocess RPC/HTTP and/or PyO3/FFI with clear trade-offs.
4. **Explicit context injection**: minimize hidden globals; inject config/workspace/http/tool backends via constructors.

## 5. Related docs

- **`docs/en/DEVELOPMENT.md`**: high-level module map, layering, and where to update when contracts change.
- **`.cursor/rules/cli-tui-web-shared-logic.mdc`**: avoid diverging business rules across interfaces.
- **`docs/en/SSE_PROTOCOL.md`**: streaming contract; any new non-HTTP embedding API should define mapping rules explicitly.

---

If crate split or new FFI/RPC layers are implemented, update this document and keep the architecture sections in **`docs/开发文档.md`** / **`docs/en/DEVELOPMENT.md`** in sync.
