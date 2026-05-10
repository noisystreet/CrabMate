**Language:** English · [中文（主文档）](../调试指南.md)

# Debugging and troubleshooting

This page mirrors [docs/调试指南.md](../调试指南.md) in English. For the full, authoritative checklist (tables, env vars, and cross-links), prefer the Chinese document unless you only need a short pointer.

## Highlights

- **Web UI env vars (`GET /web-ui`)**: **`CM_WEB_DISABLE_MARKDOWN`** sets **`markdown_render: false`** (plain-text bubbles + changelog modal). **`CM_WEB_RAW_ASSISTANT_OUTPUT`** sets **`apply_assistant_display_filters: false`** (no assistant display rewriting; same text for search/copy/export) **and** allows staged **no-tools planner** rounds to stream verbatim to the browser; when unset (default), planner SSE is gated: if the plan JSON parses with **`no_task: true`**, the whole planner round is omitted from Web SSE; otherwise only pre-`assistant_answer_phase` streaming deltas are dropped. **No TOML keys**; truthy = `1`/`true`/`yes`/`on`; **restart `serve`**; on fetch failure the CSR keeps defaults (Markdown on, filters on). Details in [docs/调试指南.md](../调试指南.md) §1.
- **Logging**: **`tracing`** + **`tracing-subscriber`** (existing **`log::`** calls bridged via **`tracing-log`**). **`RUST_LOG`** uses the same filter syntax as before. **`CM_LOG_JSON=1`** enables JSON lines. Web stream jobs can carry root span fields aligned with **`x-stream-job-id`** / SSE **`job_id`**. Defaults differ by subcommand; use `crabmate=debug` or `crabmate::message_pipeline=trace` for context pipeline lines. Global **`--log <FILE>`** mirrors to stderr. See [docs/调试指南.md](../调试指南.md) §2, [docs/en/CLI.md](CLI.md), and [docs/en/DEVELOPMENT.md](DEVELOPMENT.md).
- **CLI**: **`crabmate doctor`** needs no `API_KEY`; **`probe`** / **`models`** usually do under bearer auth. **`save-session`**, **`tool-replay`** help offline inspection.
- **HTTP**: **`/health`**, **`/status`**, **`/web-ui`**, **`/openapi.json`** — see [docs/en/CLI.md](CLI.md) for the route table.
- **Tool**: **`diagnostic_summary`** — redacted diagnostics only; **never** pastes env values. See [docs/en/TOOLS.md](TOOLS.md).
- **SSE**: [docs/SSE协议.md](../SSE协议.md); keep **`crabmate-sse-protocol`** (`control_classify`), **`sse_dispatch`**, and **`fixtures/sse_control_golden.jsonl`** in sync when changing control JSON.
- **Replay dump (`CM_REPLAY_DUMP_DIR`)**: JSONL lines include **`replay_schema_version`** (currently `1`) and per-turn **`replay_turn_seq`**; lines are redacted before append. See [docs/调试指南.md](../调试指南.md) §9.

**Secrets**: Do not paste real API keys or tokens into logs or issues; see project rules under **`.cursor/rules/secrets-and-logging.mdc`**.
