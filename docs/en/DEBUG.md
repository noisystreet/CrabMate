**Language:** English · [中文（主文档）](../DEBUG.md)

# Debugging and troubleshooting

This page mirrors [docs/DEBUG.md](../DEBUG.md) in English. For the full, authoritative checklist (tables, env vars, and cross-links), prefer the Chinese document unless you only need a short pointer.

## Highlights

- **`AGENT_WEB_DISABLE_MARKDOWN`**: When set to a truthy value (`1`, `true`, `yes`, `on`), the CSR loads **`GET /web-ui`** and turns off Markdown for assistant bubbles and the workspace changelog modal (escaped plain text). **No TOML key**; **restart `serve`** after changing.
- **Logging**: `RUST_LOG` defaults differ by subcommand; use `crabmate=debug` or `crabmate::message_pipeline=trace` for context pipeline lines. Global **`--log <FILE>`** mirrors to stderr. See [docs/en/CLI.md](CLI.md) and [docs/en/DEVELOPMENT.md](DEVELOPMENT.md).
- **CLI**: **`crabmate doctor`** needs no `API_KEY`; **`probe`** / **`models`** usually do under bearer auth. **`save-session`**, **`tool-replay`** help offline inspection.
- **HTTP**: **`/health`**, **`/status`**, **`/web-ui`**, **`/openapi.json`** — see [docs/en/CLI.md](CLI.md) for the route table.
- **Tool**: **`diagnostic_summary`** — redacted diagnostics only; **never** pastes env values. See [docs/en/TOOLS.md](TOOLS.md).
- **SSE**: [docs/SSE_PROTOCOL.md](../SSE_PROTOCOL.md); keep **`control_dispatch_mirror`**, **`sse_dispatch`**, and **`fixtures/sse_control_golden.jsonl`** in sync when changing control JSON.

**Secrets**: Do not paste real API keys or tokens into logs or issues; see project rules under **`.cursor/rules/secrets-and-logging.mdc`**.
