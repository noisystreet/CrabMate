**Languages / 语言:** [中文](../命令行契约.md) · English (this page)

# CLI contract (exit codes, JSON, ops subcommands)

For scripts and CI: aligned with `src/runtime/cli_exit.rs`, `crates/crabmate-config` clap, and **after_help** in `crabmate --help`. Streaming **Web** error codes: [SSE_PROTOCOL.md](SSE_PROTOCOL.md) § Stream error `code` enum.

> **D2.1**: in-process **`crabmate chat`** entry is removed. Sections below about `chat` exit codes / `--output json` are **historical** (old scripts/logs). Prefer Client **`crabmate-tui`**, HTTP APIs, or this repo’s **`tool-replay`** / **`bench`**.

## Process exit codes (historical: `chat`; current: `tool-replay`, etc.)

| Code | Meaning | Typical case |
|------|---------|--------------|
| 0 | Success | Turn completed without “all denied” branch |
| 1 | General error | I/O, config, uncategorized failure |
| 2 | Usage / input | Bad args, JSON/JSONL parse failure |
| 3 | Model / parse | Gateway error body, unparsable response, some invalid plan prefix (heuristic `classify_model_error_message`) |
| 4 | All `run_command` attempts denied this turn (**historical**) | In-process `chat` pipe without `y`/`a`, or interactive all-deny. **Not emitted in production after D2.2** (no in-process CLI tool approval); constant kept for contract tests |
| 5 | Quota / rate limit | HTTP 429, 402, some 503 (heuristic) |
| 6 | Tool replay mismatch | `tool-replay run --compare-recorded` string mismatch vs `recorded_output` |

Constants: `EXIT_GENERAL`, `EXIT_USAGE`, `EXIT_MODEL_ERROR`, `EXIT_TOOLS_ALL_RUN_COMMAND_DENIED` (**historical**, not emitted in production after D2.2), `EXIT_QUOTA_OR_RATE_LIMIT`, `EXIT_TOOL_REPLAY_MISMATCH` in `src/runtime/cli_exit.rs` / `crates/crabmate-runtime`. Tests: `tests/cli_contract.rs` (still asserts placeholder value 4).

## SSE / stream error codes (Web `POST /chat/stream`)

Control-plane JSON with **`error` + non-empty `code`** signals stream-level failure (distinct from model text containing `{"error":"…"}`). Common **`code`** values: [SSE_PROTOCOL.md](SSE_PROTOCOL.md). Examples:

| `code` | Summary |
|--------|---------|
| `INTERNAL_ERROR` | Queue or other orchestration failure (**`error`** generic user text; **`reason_code`** truncated internal summary) |
| `STEP_RETRY_EXHAUSTED` / `REPLAN_EXHAUSTED` / `TIME_LIMIT_EXHAUSTED` / `TOKEN_LIMIT_EXHAUSTED` | Orchestration budget failures (same shape) |
| `CONVERSATION_CONFLICT` | Conversation revision conflict |
| `plan_rewrite_exhausted` | Final plan rewrite budget exhausted (optional `reason_code`; see `docs/en/SSE_PROTOCOL.md`) |
| `SSE_ENCODE` | Control JSON serialization failure (fallback) |

**`INTERNAL_ERROR`** and related codes may appear on **SSE** and on **`POST /chat` JSON** from the same `RunAgentTurnError` mapping; `chat` subprocesses still use `classify_model_error_message` on error strings.

**HTTP JSON (not SSE `data:`)**: failed responses use **`ApiError`** with **`code`**, **`message`** (user-facing); optional **`reason_code`** (often a truncated internal summary for **`INTERNAL_ERROR`**); optional **`request_id`** (same value as response header **`x-request-id`**; filled by middleware for **4xx/5xx** `application/json` **`ApiError`** bodies when missing); optional **`details[]`** (field-level subcodes; old clients may ignore). **SSE** may still attach **`reason_code`** under multiple stream `code` values (see **`docs/en/SSE_PROTOCOL.md`**). Handshake-stage codes: **`web/chat_handlers`** and OpenAPI.

### HTTP `ApiError.code` table (non-SSE)

Prefer response header **`x-request-id`** for correlation. Common stable codes (not exhaustive). **Retryable** means automatic client backoff is reasonable (changing credentials is not “auto-retry”).

| `code` | Typical HTTP | Retryable | Summary |
|--------|--------------|-----------|---------|
| `UNAUTHORIZED` | 401 | No | Bearer / X-API-Key failure |
| `QUEUE_FULL` | 503 | **Yes** (backoff) | Chat queue full |
| `LLM_API_KEY_REQUIRED` | 400 | No | Missing model API key |
| `WORKSPACE_NOT_SET` | 400 | No | Workspace unset |
| `SSE_PROTOCOL_MISMATCH` / `SSE_CLIENT_TOO_NEW` / `INVALID_SSE_CLIENT_PROTOCOL` | 400 | No | SSE handshake |
| `STREAM_JOB_GONE` | 410 | No | Stream job finished (`stream_resume` and `POST /chat/stream/{job_id}/cancel`) |
| `UNKNOWN_JOB` | 404 | No | Unknown async `job_id` |
| `CONVERSATION_NOT_FOUND` / `INVALID_CONVERSATION_ID` | 404 / 400 | No | Conversation |
| `CONVERSATION_CONFLICT` / `CONVERSATION_REVISION_UNKNOWN` | 409 / 400 | No | Optimistic lock |
| `APPROVAL_*` / `INVALID_APPROVAL_*` | 4xx | No | Tool approval |
| `CONFIG_RELOAD_FAILED` / `SESSION_STORE_SWITCH_FAILED` | 400 | No | Config / session store |
| `CLONE_*` / `CLONE_AUTH_REQUIRED` | 4xx / 429 | Sometimes (`BUSY`) | Workspace-pool clone |
| `SKILL_INVOKE_FAILED` | 400 | No | Forced skill invoke |
| `UPLOAD_*` / `MULTIPART_ERROR` | 4xx | No | Upload |
| `EMPTY_MESSAGE` / `INVALID_*` | 400 | No | Validation |
| `INTERNAL_ERROR` | 500 | Sometimes | Orchestration; may include truncated `reason_code` |
| Budget / LLM codes (`STEP_RETRY_EXHAUSTED`, `LLM_RATE_LIMIT`, …) | 4xx / 5xx | Sometimes | See SSE docs for stream path |

Constants: **`crates/crabmate-api-contract/src/error_codes.rs`**. Full streaming error table: **[`docs/en/SSE_PROTOCOL.md`](SSE_PROTOCOL.md)**.

### HTTP / OpenAPI contract semver and external pins

- DTOs and schemars live in **`crabmate-api-contract`**; machine-readable **`GET /openapi.json`**.
- Crate semver vs SSE wire protocol, compatibility window, and git tag **`client-contract-vX.Y.Z`**: **[`docs/design/client_contract_versioning.md`](../design/client_contract_versioning.md)**.
- Gate: `bash scripts/check-client-contract.sh` (OpenAPI smoke + external-style path consumer).

## `chat --output json` one JSON line per turn (historical; entry removed)

> **Removed**: `crabmate chat --output json` is no longer available. Fields below describe the old stdout JSON line for migration only.

### Top-level fields

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Always **`crabmate_chat_cli_result`** |
| `v` | number | Schema version, currently **`1`** |
| `reply` | string | Last assistant `content` this turn (empty if none) |
| `model` | string | Current configured model id |
| `batch_line` | number? | Only with **`--message-file`**: 1-based JSONL line number |

### Examples

Single turn:

```json
{"type":"crabmate_chat_cli_result","v":1,"reply":"Hello.","model":"deepseek-chat"}
```

Batch line:

```json
{"type":"crabmate_chat_cli_result","v":1,"reply":"…","model":"deepseek-chat","batch_line":3}
```

### Evolution

Additive fields should keep **`v`** backward compatible or bump **`v`**; breaking changes must be documented here and in **`crabmate --help`** cross-links.

## Related docs

- Subcommands and flags: [CLI.md](CLI.md)
- Streaming protocol and `tool_result`: [SSE_PROTOCOL.md](SSE_PROTOCOL.md)
