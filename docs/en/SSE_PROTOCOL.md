**Languages / 语言:** [中文](../SSE_PROTOCOL.md) · English (this page)

# Agent SSE control-plane protocol (`/chat/stream`)

This document describes **control-plane JSON** sent by the CrabMate server on SSE `data:` lines, distinct from **plain-text model deltas**. **Payload shapes** are defined in Rust `src/sse/protocol.rs`; the **numeric protocol version** is shared with the Leptos UI via workspace crate **`crabmate-sse-protocol`** (constant **`SSE_PROTOCOL_VERSION`**, re-exported from `sse::protocol`). The browser consumes via `frontend-leptos/src/sse_dispatch.rs` (called by `frontend-leptos/src/api.rs`). Rust line classification: `src/sse/line.rs` (`classify_agent_sse_line`), semantics must match this doc.

## Protocol version `v` and negotiation

- Each control JSON object **should** include top-level **`v`** (`u8`). Current value **`1`**, aligned with **`crabmate_sse_protocol::SSE_PROTOCOL_VERSION`**.
- **Default**: Legacy payloads may omit `v`; deserialization treats missing as **`SSE_PROTOCOL_VERSION`** (`SseMessage` `#[serde(default = "default_sse_v")]`).
- **Request body (optional)**: JSON for **`POST /chat`** and **`POST /chat/stream`** may include **`client_sse_protocol`** (`u8`). If **omitted**, the server does not reject on that basis. If **`client_sse_protocol` > server `SSE_PROTOCOL_VERSION`** → **HTTP 400**, `ApiError.code` **`SSE_CLIENT_TOO_NEW`**; if **`0`** → **`INVALID_SSE_CLIENT_PROTOCOL`**.
- **First frame**: After a new stream is attached, the server emits **`sse_capabilities`** with **`supported_sse_v`** equal to server **`SSE_PROTOCOL_VERSION`**. The official Leptos client compares to its compile-time constant; on mismatch it calls `onError` and stops reading; the message includes **`SSE_SERVER_TOO_NEW`** (server newer, client older) or **`SSE_SERVER_TOO_OLD`** (server older; usually already rejected by **`SSE_CLIENT_TOO_NEW`**).
- **Evolution**: Bump **`crates/crabmate-sse-protocol`**, this doc and the Chinese twin, and run **`cargo test -p crabmate-sse-protocol`** (doc marker self-check).

## Transport and framing

- **Route**: **`POST /chat/stream`**; response **`text/event-stream`**. (Ops **`POST /config/reload`** is JSON, not SSE—see **CONFIGURATION.md** § hot reload.)
- **Event `id:`**: Each logical block has monotonic **`id:`** (`u64`, in-process hub). Reconnect with header **`Last-Event-ID`** and JSON **`stream_resume`**: `{ "job_id": <u64>, "after_seq": <u64> }` (omit `after_seq` → 0). Server uses **`max(Last-Event-ID, after_seq)`**, replays from the ring buffer, then subscribes to live broadcast. **In-process only**: after the job ends or the process restarts, reconnect returns **HTTP 410** with **`STREAM_JOB_GONE`**. New streams also expose **`x-stream-job-id`** (same as first-frame `sse_capabilities.caps.job_id`).
- **Event blocks**: Separated by **blank line `\n\n`**; each block may contain multiple **`data: `** lines. The frontend **joins** same-block `data:` lines with `\n`, then `trim()`, before parsing (see `sendChatStream`).
- **Text delta**: If the joined string is **not** valid control JSON, or parses as **`plain`**, it is treated as assistant content for `onDelta`.
- **Stream end**: Literal **`[DONE]`** may appear (OpenAI-style); frontend ignores it as content. See also **`stream_ended`**.

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
| `assistant_answer_phase`: `true` | Following plain-text deltas are assistant **answer** `content` (previously reasoning); emitted before the first content chunk even when there is no reasoning chain | Web: **handled**; route `onDelta` to reasoning vs answer buffer, not as raw prose |
| `staged_plan_started` | Staged plan start | `onStagedPlanStarted` |
| `staged_plan_step_started` | Step start; body has `plan_id`, `step_id`, `step_index`, `total_steps`, `description`, optional `executor_kind` (`review_readonly` / `patch_write` / `test_runner`) | `onStagedPlanStepStarted` |
| `staged_plan_step_finished` | Step end; `status`: `ok` / `cancelled` / `failed`; optional `executor_kind` (mirrors `staged_plan_step_started`) | `onStagedPlanStepFinished` |
| `staged_plan_finished` | Whole plan end | `onStagedPlanFinished` |
| `clarification_questionnaire` | Clarification form: after a successful **`present_clarification_questionnaire`** tool run, emitted **after** the `tool_result` SSE; body has **`questionnaire_id`**, **`intro`**, **`questions[]`** (`id` / `label` / optional `hint` / `required` / `kind`: `text` \| `choice`) | Web: show form; next request includes **`clarify_questionnaire_answers`**; TUI: `line` treats as **ignore** |
| `workspace_changed`: `true` | Tools updated workspace | `onWorkspaceChanged` |
| `tool_call` | Tool call summary (before run); body has **`name`**, **`summary`** (same source as `summarize_tool_call`), optional **`arguments_preview`** (single-line truncation, aligned with `execute_tools` logs), optional **`arguments`** when **`sse_tool_call_include_arguments`** / **`AGENT_SSE_TOOL_CALL_INCLUDE_ARGUMENTS`** is on (heuristically redacted, longer cap) | `onToolCall` (**handled** if any of **`summary`**, **`arguments_preview`**, **`arguments`** is non-empty) |
| `parsing_tool_calls` | Model streaming tool_calls | `onParsingToolCallsChange` |
| `tool_running` | Tool running | `onToolStatusChange` |
| `tool_result` | Tool finished; includes `output` | `onToolResult` |
| `command_approval_request` | Approval for `run_command` / workflow | `onCommandApprovalRequest` |
| `staged_plan_notice` / `staged_plan_notice_clear` | Plan progress text; Web **swallows** | `handled`, not `onDelta` |
| `chat_ui_separator` | UI separator; `true` short, `false` long | `onChatUiSeparator` |
| `conversation_saved` | Session persisted; `revision` for branching/conflict | `onConversationSaved` |
| `sse_capabilities` | First frame: `supported_sse_v`, `resume_ring_cap`, `job_id` (matches `x-stream-job-id`) | Official Web: compare to local **`SSE_PROTOCOL_VERSION`**; if match, **swallow**; else **`onError`** and stop. Integrations can persist `job_id` for resume |
| `stream_ended` | End of stream; `job_id`, `reason` (`completed` / `cancelled`) | Web: **swallow**; clients may stop auto-reconnect |
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
| `failure_category` | string? | Coarse bucket aligned with Rust **`tool_result::ToolFailureCategory::as_str`** and history **`crabmate_tool.failure_category`**; derived from **`error_code`** (omitted on success). Stable values: **`failure_category` enum** below |
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

**`SsePayload::Error`** on SSE `data:` lines (`error` + non-empty `code`). Distinct from model text that only has `error` without `code` (see above).

| `code` | Source | Meaning |
|--------|--------|---------|
| `CONVERSATION_CONFLICT` | `web/chat_handlers/conflict`, `chat_job_queue` | Session revision / save conflict |
| `INTERNAL_ERROR` | `chat_job_queue` | `run_agent_turn` failure (non-cancel), user-facing fallback text; **`reason_code`** may carry a truncated internal summary |
| `LLM_REQUEST_FAILED` | `chat_job_queue` (mapped from `agent_turn`) | Model HTTP/transport failure (**`error`** is the redacted gateway message; prefer **`LLM_RATE_LIMIT`** for **429** / quota heuristics) |
| `LLM_RATE_LIMIT` | `chat_job_queue` (mapped from `agent_turn`) | Rate limit / quota class (**HTTP 429** or heuristic aligned with `agent_errors::is_quota_or_rate_limit_llm_message`) |
| `turn_aborted` | `chat_job_queue` (mapped from `agent_turn`) | Orchestration early stop (e.g. SSE receiver closed while the turn continues); **`error`** is user-facing |
| `STREAM_CANCELLED` | `chat_job_queue` | Cancelled stream, delivered when channel still open |
| `plan_rewrite_exhausted` | `agent_turn/outer_loop`, `agent_turn/staged` | Final plan rewrite budget exhausted |
| `SSE_ENCODE` | `sse/protocol` | `encode_message` serialization fallback |

**Optional `reason_code`**: sibling string sub-code for client branching under the same top-level `code` (currently used for `plan_rewrite_exhausted`); older clients may ignore it.

**Optional `turn_id`**: matches **`x-stream-job-id`** and **`sse_capabilities.job_id`** (`u64`); omitted on non-Web paths or legacy frames.

**Optional `sub_phase`**: orchestration sub-phase at failure, aligned with PER: `planner` \| `executor` \| `reflect`; older clients may ignore it.

#### `reason_code` for `plan_rewrite_exhausted`

Approximate category of the **last** failed final answer when the rewrite budget is exhausted.

| `reason_code` | Meaning |
|----------------|---------|
| `plan_missing` | No parseable `agent_reply_plan` v1 in content |
| `plan_layer_count_mismatch` | Fewer `steps` than required `workflow_validate` `layer_count` |
| `plan_workflow_node_ids_invalid` | `workflow_node_id` not consistent with latest workflow node ids |
| `plan_workflow_node_coverage_incomplete` | Strict mode: not all workflow node ids covered |
| `plan_validate_only_node_binding_mismatch` | After `workflow_validate_only`, plan steps do not bind 1:1 to `nodes[].id` (count, required `workflow_node_id`, multiset) |
| `plan_semantic_inconsistent` | Side semantic check disagrees with recent tool output |
| `plan_rewrite_exhausted_other` | Defensive fallback (should not occur on main paths) |

**HTTP only** (JSON `ApiError`, not SSE `data:`), stream-related extras:

| `code` | HTTP | Notes |
|--------|------|------|
| `STREAM_JOB_GONE` | 410 | **`stream_resume`** job not in hub |
| `SSE_CLIENT_TOO_NEW` | 400 | **`client_sse_protocol`** greater than server **`SSE_PROTOCOL_VERSION`** |
| `INVALID_SSE_CLIENT_PROTOCOL` | 400 | **`client_sse_protocol == 0`** |
| `INVALID_AT_FILE_REF` | 400 | User message contains an invalid **`@…`** file reference (e.g. absolute path or **`/`**-prefixed “pseudo-relative”); must be relative to the workspace root, same rules as **`read_file`** |
| `INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS` | 400 | Invalid **`clarify_questionnaire_answers`** payload (`questionnaire_id` / `answers` keys and size limits); see `clarification_questionnaire` module |

**Client-only hints** (in `onError` text from official Leptos when **`sse_capabilities`** disagrees): **`SSE_SERVER_TOO_NEW`**, **`SSE_SERVER_TOO_OLD`**.

**Legacy** (rarely emitted today): `staged_plan_tool_calls`, `staged_plan_invalid` — see `chat_job_queue` logging for `staged_plan_invalid:` prefix errors.

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

Full heuristics: `src/tool_result/mod.rs` (`classify_error_code`); **`error_code` → `failure_category`**: `src/tool_result/tool_error.rs` (**`failure_category_for_error_code`**, matches **`ToolFailureCategory`**). Workflow-specific: `src/agent/workflow/execute.rs`.

### `tool_result.failure_category` (and `crabmate_tool.failure_category`)

Same strings as Rust **`tool_result::ToolFailureCategory::as_str()`** so clients can **`match`** without overfitting **`error_code`** free-form strings:

| `failure_category` | Meaning |
|--------------------|---------|
| `invalid_input` | Args / JSON / required fields |
| `policy_denied` | Allowlist, rate limits, policy |
| `workspace` | Workspace not set, path outside allowed roots |
| `timeout` | Tool or subprocess timeout |
| `external` | Non-zero exit, IO, HTTP business failure |
| `internal` | Rare internal invariant |
| `unknown` | Unclassified or unknown tool |

New **`error_code`** values may map to **`unknown`** or fall through the `_failed` suffix rule to **`external`** until **`failure_category_for_error_code`** is extended.

## vs `POST /chat` HTTP errors

Queue full, auth failures, etc. return **HTTP 4xx/5xx + JSON** (e.g. `code: "QUEUE_FULL"`), **not** SSE `data:` lines. The full **`ApiError.code`** table lives in **`docs/CLI_CONTRACT.md`**; **this doc** focuses on SSE control-plane and **`client_sse_protocol`**-related HTTP codes, complementing the stream error table above.

## Dual-end checklist

When changing any of:

1. **`crates/crabmate-sse-protocol`**: **`SSE_PROTOCOL_VERSION`**; `src/sse/protocol.rs`: `SsePayload`, `SseErrorBody`, `ToolResultBody` (version from the crate, re-exported in `protocol`)
2. `frontend-leptos/src/sse_dispatch.rs` and `frontend-leptos/src/api.rs`: classification order and **`client_sse_protocol`** in the request body
3. `src/sse/line.rs`: `classify_agent_sse_line`
4. New `encode_message(SsePayload::…)` call sites

…keep Rust, Leptos, and this doc aligned.

## Contract tests (control-plane classification)

After parsing one merged `data:` string as JSON, the frontend applies a **fixed order** to decide `stop` / `handled` / `plain` (`frontend-leptos/src/sse_dispatch.rs`). The **single source of truth** is **`classify_sse_control_outcome`** in workspace crate **`crates/crabmate-sse-protocol`** (`control_classify.rs`), aligned with the same golden file; Leptos also runs **`golden_sse_control_leptos_dispatch_matches_shared_classify`** to catch drift.

- **`fixtures/sse_control_golden.jsonl`**: each line `description<TAB>JSON<TAB>expected-class` (`#` lines are comments).
- **Rust**: `cargo test golden_sse_control` (runs **`crabmate-sse-protocol`** golden tests plus **frontend-leptos** alignment).
When adding a new top-level key consumed by the Web UI: update `frontend-leptos/src/sse_dispatch.rs`, **`crates/crabmate-sse-protocol/control_classify.rs`**, and golden lines.

## Contract tests (`crabmate_tool` history envelope)

- **`fixtures/tool_result_envelope_golden.jsonl`**: each line `description<TAB>single-line JSON` (`#` lines are comments); round-trip via **`tool_result::normalize_tool_message_content`** + **`NormalizedToolEnvelope::encode_to_message_line`**.
- **Rust**: `cargo test tool_result_envelope_golden`.

---

Maintainers: tables should match code; if they drift, treat **`protocol.rs` + `frontend-leptos/src/sse_dispatch.rs`** as authoritative and fix this doc.
