**Languages / 语言:** [中文](../CLI_CONTRACT.md) · English (this page)

# CLI contract (exit codes, JSON, `chat` output)

For scripts and CI: aligned with `src/runtime/cli_exit.rs`, `src/config/cli.rs` (clap), and **after_help** in `crabmate --help`. Streaming **Web** error codes: [SSE_PROTOCOL.md](SSE_PROTOCOL.md) § Stream error `code` enum.

## `chat` process exit codes

| Code | Meaning | Typical case |
|------|---------|--------------|
| 0 | Success | Turn completed without “all denied” branch |
| 1 | General error | I/O, config, uncategorized failure |
| 2 | Usage / input | Bad args, JSON/JSONL parse failure |
| 3 | Model / parse | Gateway error body, unparseable response, some invalid plan prefix (heuristic `classify_model_error_message`) |
| 4 | All `run_command` attempts denied this turn | Pipe without `y`/`a`, or interactive all-deny |
| 5 | Quota / rate limit | HTTP 429, 402, some 503 (heuristic) |
| 6 | Tool replay mismatch | `tool-replay run --compare-recorded` string mismatch vs `recorded_output` |

Constants: `EXIT_GENERAL`, `EXIT_USAGE`, `EXIT_MODEL_ERROR`, `EXIT_TOOLS_ALL_RUN_COMMAND_DENIED`, `EXIT_QUOTA_OR_RATE_LIMIT`, `EXIT_TOOL_REPLAY_MISMATCH` in `src/runtime/cli_exit.rs`. Tests: `tests/cli_contract.rs`.

## SSE / stream error codes (Web `POST /chat/stream`)

Control-plane JSON with **`error` + non-empty `code`** signals stream-level failure (distinct from model text containing `{"error":"…"}`). Common **`code`** values: [SSE_PROTOCOL.md](SSE_PROTOCOL.md). Examples:

| `code` | Summary |
|--------|---------|
| `INTERNAL_ERROR` | Queue or unexpected internal error |
| `CONVERSATION_CONFLICT` | Conversation revision conflict |
| `plan_rewrite_exhausted` | Final plan rewrite budget exhausted |
| `SSE_ENCODE` | Control JSON serialization failure (fallback) |

**`INTERNAL_ERROR`** appears only on **SSE**; it is **not** mapped to the numeric `chat` exit codes above; `chat` failures still use `classify_model_error_message` on error strings.

## `chat --output json` one JSON line per turn (stable shape)

After each turn, **stdout** prints **one** UTF-8 JSON line for `jq` / scripts.

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
