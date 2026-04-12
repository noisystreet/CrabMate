**Language:** English · [中文（主文档）](../DEBUG.md)

# Debugging and troubleshooting

This page mirrors [docs/DEBUG.md](../DEBUG.md) in English. For the full, authoritative checklist (tables, env vars, and cross-links), prefer the Chinese document unless you only need a short pointer.

## Highlights

- **Web UI env vars (`GET /web-ui`)**: **`AGENT_WEB_DISABLE_MARKDOWN`** sets **`markdown_render: false`** (plain-text bubbles + changelog modal). **`AGENT_WEB_RAW_ASSISTANT_OUTPUT`** sets **`apply_assistant_display_filters: false`** (no assistant display rewriting; same text for search/copy/export) **and** allows staged **no-tools planner** rounds to stream verbatim to the browser; when unset (default), planner SSE is gated: if the plan JSON parses with **`no_task: true`**, the whole planner round is omitted from Web SSE; otherwise only pre-`assistant_answer_phase` streaming deltas are dropped. **No TOML keys**; truthy = `1`/`true`/`yes`/`on`; **restart `serve`**; on fetch failure the CSR keeps defaults (Markdown on, filters on). Details in [docs/DEBUG.md](../DEBUG.md) §1.
- **Logging**: **`tracing`** + **`tracing-subscriber`** (existing **`log::`** calls bridged via **`tracing-log`**). **`RUST_LOG`** uses the same filter syntax as before. **`AGENT_LOG_JSON=1`** enables JSON lines. Web stream jobs can carry root span fields aligned with **`x-stream-job-id`** / SSE **`job_id`**. Defaults differ by subcommand; use `crabmate=debug` or `crabmate::message_pipeline=trace` for context pipeline lines. Global **`--log <FILE>`** mirrors to stderr. See [docs/DEBUG.md](../DEBUG.md) §2, [docs/en/CLI.md](CLI.md), and [docs/en/DEVELOPMENT.md](DEVELOPMENT.md).
- **CLI**: **`crabmate doctor`** needs no `API_KEY`; **`probe`** / **`models`** usually do under bearer auth. **`save-session`**, **`tool-replay`** help offline inspection.
- **HTTP**: **`/health`**, **`/status`**, **`/web-ui`**, **`/openapi.json`** — see [docs/en/CLI.md](CLI.md) for the route table.
- **Tool**: **`diagnostic_summary`** — redacted diagnostics only; **never** pastes env values. See [docs/en/TOOLS.md](TOOLS.md).
- **SSE**: [docs/SSE_PROTOCOL.md](../SSE_PROTOCOL.md); keep **`crabmate-sse-protocol`** (`control_classify`), **`sse_dispatch`**, and **`fixtures/sse_control_golden.jsonl`** in sync when changing control JSON.

**Secrets**: Do not paste real API keys or tokens into logs or issues; see project rules under **`.cursor/rules/secrets-and-logging.mdc`**.
