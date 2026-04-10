**Languages / 语言:** [中文](../TODOLIST.md) · English (this page)

# Backlog and improvement list

This file lists **only open** work items. **Remove an item when it is done** (do not keep `[x]` long-term); delete empty section headings. Use Git history to see when something was completed. Maintenance rules: see [DEVELOPMENT.md](DEVELOPMENT.md) § TODOLIST conventions.

**Structure**:

- **Global priorities (cross-cutting)**: P0–P5; may overlap module chapters.
- **By module**: One chapter per area (`agent/`, `llm/`, `tools/`, etc.) with a short responsibility summary at the top.

---

## Global priorities

### P0 — Security (before non-localhost deployment)

- [ ] **Unauthenticated HTTP**: `/chat`, `/chat/stream`, workspace, files, upload, tasks do not verify caller identity; `API_KEY` is only for the LLM, not for protecting APIs or quota abuse.
- [ ] **Multi-role / persona switching**: Support multiple **roles** (system prompt, tool visibility, temperature, etc.); **CLI** and **Web** should expose commands or UI to switch the active role per session, with boundaries documented for persistence, export, and `POST /config/reload`; multi-tenant use must align with authentication above.

### P4 — Testing and quality

- [ ] **Unified error types (incremental)**: Reduce plain `String` / `format!` on hot paths; structured enums for `run_command`, path resolution, etc.
- [ ] **Production `unwrap`/`expect` audit**: Replace with explicit propagation or documented `expect`.
- [ ] **Integration / contract tests**: Extend beyond `lib_smoke` and `tests/cli_contract.rs` for `plan_artifact`, `classify_agent_sse_line`, workflow reflection state.
- [ ] **`stream_chat` non-streaming tests**: Optional wiremock / static JSON fixtures for `ChatResponse`.
- [ ] **Agent benchmarks**: Systematic runs on SWE-bench, HumanEval, GAIA, etc.; batch harness exists (`--benchmark` + `--batch`).

### P5 — Operations and UX

- [ ] **Cross-process / multi-replica queue**: Today single-process `mpsc` + `Semaphore`; horizontal scale needs Redis/SQS etc.
- [ ] **Rate limits / quotas**: For `/chat`, `/chat/stream` by IP or token (often with P0 auth).
- [ ] **Log correlation**: Unify `request_id` / `conversation_id` once session model lands.

---

## `agent/` (turn loop, context, PER, workflow)

**Summary**: `agent_turn` main loop; `context_window` trim/summarize; `per_coord` / `plan_artifact` / `workflow_reflection_controller`; `workflow` DAG execution.

(Items mirror the Chinese [TODOLIST.md](../TODOLIST.md): agent_turn/llm boundaries, planner/executor phases, PER plug-in, long-term memory external vector DB, hybrid retrieval, TTL/dedup, compliance APIs; codebase index follow-ups—FTS/hybrid beyond incremental `codebase_semantic_search`—per [CODEBASE_INDEX_PLAN.md](CODEBASE_INDEX_PLAN.md).)

---

## `llm/` and `http_client.rs`

**Summary**: `ChatRequest`, `complete_chat_retrying`; SSE/JSON parsing in `api`; shared `reqwest::Client`.

- [ ] Optional upstream metrics (HTTP status / retryable dimensions).
- [ ] Optional token/cost estimates vs `context_window` budget.
- [ ] Non-stream vs stream contract tests.
- [ ] Optional TLS/connection debug logging (no full sensitive URLs).

---

## `tools/` and `tool_registry.rs`

**Summary**: Table-driven `ToolSpec`, `run_tool`; workflow / timeouts / search policies in `tool_registry`.

- [ ] Extend sensitive-operation tiers and `tool_approval::SensitiveCapability` for disk writes if needed.
- [ ] More stacks under `dev_tag` as needed (whitelist and path safety).
- [ ] MCP (continued): Streamable HTTP/SSE clients, multi-server, auth; optional HTTP (streamable) server aligned with Web auth. Stdio server: **`crabmate mcp serve`** (see **`docs/en/CLI.md`**). Keep documenting boundaries vs `run_command` / workspace policy.

---

## `sse/`

**Summary**: `protocol` encodes control JSON; `line` classifies lines (align with `frontend-leptos`).

- [ ] Optional debug payloads (dev-only).

---

## `lib.rs`, `chat_job_queue`, `web/`

**Summary**: Axum router, `AppState`, chat queue, workspace/task APIs.

- [ ] Auth and multi-tenant isolation (with P0).
- [ ] Messages / `conversation_id` API aligned with `run_agent_turn`.
- [ ] Upload quotas and retention.

---

## `config/`

**Summary**: Embedded/file TOML, env, CLI merged into `AgentConfig`.

- [ ] Domain-split config assembly; clearer override order validation.
- [ ] Startup validation: unknown keys, type errors, out-of-range values.
- [ ] Optional hot reload subset (SIGHUP / file watch).
- [ ] Profiles (`dev`/`prod`).
- [ ] Secret management integration (vault, file permissions).

---

## `runtime/`

**Summary**: CLI, `workspace_session`, `chat_export`, terminal display.

- [ ] Future full-screen TUI.
- [ ] CLI history persistence, batch message injection from files.
- [ ] Export JSON schema version field.
- [ ] Accessibility and weak-terminal fallbacks.

---

## `frontend-leptos/`

**Summary**: `api.rs`, `sse_dispatch.rs`, panels, session state, `session_export`.

- [ ] Browser session state: persist `conversation_id` + `revision` across reloads (and optional encrypted cache); tab-local model is **`frontend-leptos/src/session_sync.rs`** (`SessionSyncState`).
- [ ] Virtualize long chat lists.
- [ ] i18n, a11y, keyboard navigation.
- [ ] Future voice (STT/TTS) with P0 auth alignment.

---

## Cross-cutting (`types`, `tool_result`, `health`, `redact`, `text_sanitize`)

**Summary**: OpenAI-shaped types; structured tool results; `/health`; redaction; UI sanitization.

- [ ] Request correlation ID through logs and SSE.
- [ ] Health/capacity dimensions (disk, queue depth) on top of optional **`GET /health`** **`llm_models_endpoint`** (**`health_llm_models_probe`**).
- [ ] Central rules library for `redact` and truncation when adding tools.

---

*Completed work does not belong in this file. When changing `src/` module layout, update [DEVELOPMENT.md](DEVELOPMENT.md). Security surface: `.cursor/rules/security-sensitive-surface.mdc`.*
