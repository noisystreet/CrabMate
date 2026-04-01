**Languages / 语言:** [中文](../SSE_PROTOCOL.md) · English (this page)

# Agent SSE control-plane protocol (`/chat/stream`)

This document describes **control-plane JSON** sent by the CrabMate server on SSE `data:` lines, distinct from **plain-text model deltas**. The source of truth for types is Rust `src/sse/protocol.rs`; the browser consumes via `frontend-leptos/src/sse_dispatch.rs` (called by `frontend-leptos/src/api.rs`). Rust line classification: `src/sse/line.rs` (`classify_agent_sse_line`), semantics must match this doc.

## Protocol version `v`

- Each control JSON object **should** include top-level **`v`** (`u8`). Current value **`1`**, aligned with **`sse::protocol::SSE_PROTOCOL_VERSION`**.
- **Default**: Legacy payloads may omit `v`; deserialization treats missing as **`1`** (`SseMessage` `#[serde(default = "default_sse_v")]`).
- **Evolution**: When bumping `v`, update this doc, Rust and TS constants, and handle unknown versions in the frontend (today parsing is shape-based without strict `v` checks).

## Transport and framing

- **Route**: **`POST /chat/stream`**; response **`text/event-stream`**. (Ops **`POST /config/reload`** is JSON, not SSE—see **CONFIGURATION.md** § hot reload.)
- **Event blocks**: Separated by **blank line `\n\n`**; each block may contain multiple **`data: `** lines. The frontend **joins** same-block `data:` lines with `\n`, then `trim()`, before parsing (see `sendChatStream`).
- **Text delta**: If the joined string is **not** valid control JSON, or parses as **`plain`**, it is treated as assistant content for `onDelta`.
- **Stream end**: Literal **`[DONE]`** may appear (OpenAI-style); frontend ignores it as content.

## Envelope shape

Control payloads are **single-line JSON** with logical structure:

```json
{ "v": 1, …payload… }
```

`SsePayload` uses **`serde(untagged)`**, so there is **no** outer `"SsePayload"` wrapper key; variants are distinguished by field shape (same as `SseControlPayload` in `api.ts`).

## Distinguishing from model text (`error` pitfall)

- If JSON has only **`error`** string and **`code` is missing or empty**, **do not** treat as protocol failure: model reasoning may contain example objects like `{"error":"…"}`.
- **Protocol stream errors** (stop stream, `onError`): require **non-empty `code`** (`tryDispatchSseControlPayload` / `classify_agent_sse_line`).
- Server **`encode_message` for `SsePayload::Error`** should always set non-empty `code`; serialization failure fallback uses `code: "SSE_ENCODE"`.

## Control variants (top-level keys)

These are **top-level keys** alongside `v`. Only one variant should match; parse order follows frontend `tryDispatchSseControlPayload`.

| Top-level shape | Meaning | Frontend handling |
|-----------------|---------|-------------------|
| `error` + **`code`** | Stream failure | `onError`, **stop** reading |
| `plan_required` | Reserved | `onPlanRequired`, continue |
| `staged_plan_started` | Staged plan start | `onStagedPlanStarted` |
| `staged_plan_step_started` | Step start | `onStagedPlanStepStarted` |
| `staged_plan_step_finished` | Step end; `status`: `ok` / `cancelled` / `failed` | `onStagedPlanStepFinished` |
| `staged_plan_finished` | Whole plan end | `onStagedPlanFinished` |
| `workspace_changed`: `true` | Tools updated workspace | `onWorkspaceChanged` |
| `tool_call` | Tool call summary (before run) | `onToolCall` (needs `summary`) |
| `parsing_tool_calls` | Model streaming tool_calls | `onParsingToolCallsChange` |
| `tool_running` | Tool running | `onToolStatusChange` |
| `tool_result` | Tool finished; includes `output` | `onToolResult` |
| `command_approval_request` | Approval for `run_command` / workflow | `onCommandApprovalRequest` |
| `staged_plan_notice` / `staged_plan_notice_clear` | Plan progress text; Web **swallows** | `handled`, not `onDelta` |
| `chat_ui_separator` | UI separator; `true` short, `false` long | `onChatUiSeparator` |
| `conversation_saved` | Session persisted; `revision` for branching/conflict | `onConversationSaved` |
| `timeline_log` | Timeline annotation; **not** in model context | `onTimelineLog` |

### `tool_result` common fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool name |
| `result_version` | number | **Tool-result payload version**, aligned with **`crabmate_tool.v`** in history (currently **1**). **Distinct from** top-level **`v`** (`SSE_PROTOCOL_VERSION`). Defaults to **1** if omitted. |
| `summary` | string? | Same source as `summarize_tool_call` |
| `output` | string | Full text output |
| `ok` | bool? | Success |
| `exit_code` | number? | For command-like tools |
| `error_code` | string? | Machine-readable (see below) |
| `stdout` / `stderr` | string? | Split streams if present |
| `retryable` | bool? | Heuristic on failure (e.g. timeout) |
| `tool_call_id` | string? | OpenAI `tool_calls[].id` |
| `execution_mode` | string? | `serial` or `parallel_readonly_batch` |
| `parallel_batch_id` | string? | Shared id for parallel readonly batch |

### `command_approval_request`

| Field | Description |
|-------|-------------|
| `command` | Command name |
| `args` | Argument string |
| `allowlist_key` | Optional persistent allowlist key |

## Stream error `code` enum (`error` + `code`)

SSE **stream** error codes (not HTTP JSON body). Update this table and `api.ts` when adding codes.

| `code` | Source | Meaning |
|--------|--------|---------|
| `CONVERSATION_CONFLICT` | `web/chat_handlers`, `chat_job_queue` | Session revision / save conflict |
| `INTERNAL_ERROR` | `chat_job_queue` | Queue or unexpected internal error |
| `STREAM_CANCELLED` | `chat_job_queue` | Stream cancelled (e.g. client disconnect + cooperative cancel while SSE can still deliver) |
| `staged_plan_tool_calls` | **Legacy/compat** | Rare today |
| `staged_plan_invalid` | **Legacy/compat** | Rare today |
| `plan_rewrite_exhausted` | `agent_turn/outer_loop` | Final plan rewrite budget exhausted |
| `SSE_ENCODE` | `sse/protocol` | Control JSON serialization failure |

## `tool_result.error_code` (tools / workflow)

Machine-readable failure classification (separate from stream `code`). Common values:

| `error_code` | Typical case |
|--------------|--------------|
| `invalid_args` | Argument parse error |
| `command_not_allowed` | Command not on allowlist |
| `command_denied` | User/policy denied |
| `workspace_not_set` | No workspace |
| `timeout` | Execution timeout |
| `unknown_tool` | Unknown tool name |
| `approval_required` | Awaiting approval |
| `approval_denied` | Approval denied |
| `workflow_semaphore_closed` | Workflow concurrency closed |
| `workflow_node_missing_result` | Missing node result |
| `workflow_tool_join_error` | Workflow tool task join failed |
| `{tool_name}_failed` | Generic tool failure (e.g. `run_command_failed`) |

Full heuristics: `src/tool_result/` (`classify_error_code`); workflow: `src/agent/workflow/execute.rs`.

## vs `POST /chat` HTTP errors

Queue full, auth failures, etc. return **HTTP 4xx/5xx + JSON** (e.g. `code: "QUEUE_FULL"`), **not** SSE `data:` lines. See `web/chat_handlers` and README API notes; **this file covers only SSE control-plane payloads.**

## Dual-end checklist

When changing any of:

1. `src/sse/protocol.rs`: `SsePayload`, `SseErrorBody`, `ToolResultBody`, `SSE_PROTOCOL_VERSION`
2. `frontend-leptos/src/sse_dispatch.rs` and `frontend-leptos/src/api.rs`: control-payload classification and dispatch ordering
3. `src/sse/line.rs`: `classify_agent_sse_line`
4. New `encode_message(SsePayload::…)` call sites

…keep Rust, TS, and this doc aligned.

## Contract tests (control-plane classification)

After parsing one merged `data:` string as JSON, the frontend applies a **fixed order** to decide `stop` / `handled` / `plain` (`frontend-leptos/src/sse_dispatch.rs`). Rust mirror: **`src/sse/control_dispatch_mirror.rs`** (`#[cfg(test)]`), same golden file:

- **`fixtures/sse_control_golden.jsonl`**: each line `description<TAB>JSON<TAB>expected-class` (`#` lines are comments).
- **Rust**: `cargo test golden_sse_control` or `cargo test control_dispatch_mirror`.
When adding a new top-level key consumed by the Web UI: update `frontend-leptos/src/sse_dispatch.rs`, **`control_dispatch_mirror::classify_sse_control_outcome`**, and golden lines.

## Contract tests (`crabmate_tool` history envelope)

- **`fixtures/tool_result_envelope_golden.jsonl`**: each line `description<TAB>single-line JSON` (`#` lines are comments); round-trip via **`tool_result::normalize_tool_message_content`** + **`NormalizedToolEnvelope::encode_to_message_line`**.
- **Rust**: `cargo test tool_result_envelope_golden`.

---

Maintainers: tables should match code; if they drift, treat **`protocol.rs` + `frontend-leptos/src/sse_dispatch.rs`** as authoritative and fix this doc.
