**Languages / 语言:** [中文](../CONFIGURATION.md) · English (this page)

# Configuration

Default settings are merged from seven embedded TOML fragments under **`config/`**: **`default_config.toml`**, **`session.toml`**, **`context_inject.toml`**, **`tools.toml`**, **`sandbox.toml`**, **`planning.toml`**, **`memory.toml`** (each fragment is mostly flattened under **`[agent]`**; **`config/tools.toml`** may also define optional **`[tool_registry]`**—see “`tool_registry` policy” below). **`session`** covers CLI session **`tui_*`** and **`repl_initial_workspace_messages_enabled`**; **`context_inject`** covers first-turn **`agent_memory_file_*`**, **`project_profile_inject_*`**, **`project_dependency_brief_inject_*`**; **`tools`** **`[agent]`** covers **`run_command`** allowlist/timeouts/working dir, **`tool_message_*`** / **`tool_result_envelope_v1`**, **`read_file_turn_cache_*`**, **`test_result_cache_*`**, **`session_workspace_changelist_*`**, **`codebase_semantic_*`** (the **`codebase_semantic_search`** tool), weather/search/**`http_fetch_*`**, **`tool_call_explain_*`**, **`mcp_*`**, etc.; **`sandbox`** is **SyncDefault Docker** **`sync_default_tool_sandbox_*`**; **`planning`** is planning/reflection/orchestration; **`memory`** is **`long_term_memory_*`**. `load_config` merges in order **defaults → session → context_inject → tools → sandbox → planning → memory**, then **`config.toml`** or **`.agent_demo.toml`**, then environment variables. See **`config.toml.example`** for snippets.

## Hot reload (without restarting `repl` / `serve`)

- **CLI**: Type **`/config reload`** (Tab completes). Re-reads the same config path as startup (**`--config`** or default **`config.toml`** / **`.agent_demo.toml`**), merges with **current process env**, writes hot fields into in-memory [`AgentConfig`](DEVELOPMENT.md); clears MCP stdio cache; next turn uses the new MCP fingerprint.
- **Web**: **`POST /config/reload`** (JSON body may be `{}`; same auth as **`/chat`** and other protected APIs—**`Authorization: Bearer <token>`** or **`X-API-Key: <token>`** when the layer is enabled). Success: **`{ "ok": true, "message": "…" }`**.
- **Typically hot-reloaded**: **`api_base`**, **`model`**, **`llm_http_auth_mode`**, **`llm_reasoning_split`**, **`llm_bigmodel_thinking`**, **`llm_kimi_thinking_disabled`**, **`thinking_avoid_echo_system_prompt`**, **`thinking_avoid_echo_appendix` / `thinking_avoid_echo_appendix_file`** (resolved appendix text), **`temperature` / `llm_seed`**, timeouts/retries, **`run_command`** allowlist, **`http_fetch_allowed_prefixes`**, **`workspace_allowed_roots`**, **`web_api_bearer_token`** (handler-side check only; see below), **`mcp_*`**, **`[tool_registry]`** fields (outer HTTP walls, parallel wall overrides, deny/inline/write-effect lists), **`system_prompt_file` re-read**, context/planning keys (implementation: **`apply_hot_reload_config_subset`**). **`system`→`user` folding** for MiniMax follows **`model` / `api_base`** on the next request after reload (not an `AgentConfig` field).
- **Not hot-reloaded**: **`conversation_store_sqlite_path`** (SQLite opened at startup—change path requires **`serve` restart**). **`reqwest::Client`** is not rebuilt; **`api_timeout_secs`** may lag on pooled idle connections.
- **`API_KEY`**: Still **environment only**; hot reload does not read secret files. After changing **`API_KEY`**, re-**export** and **`/config reload`** (or restart) for **`llm_http_auth_mode=bearer`** consistency.
- **Web API auth layer**: If **`serve`** started with non-empty **`web_api_bearer_token`**, the auth middleware is mounted for the process lifetime; clients send **`Authorization: Bearer <same secret>`** or **`X-API-Key: <same secret>`** (either). Hot reload **does not** add/remove the layer—switching between “no token” and “token” requires **`serve` restart**. Hot reload still updates the secret string used inside handlers when the layer exists.
- **Secrets in memory**: **`web_api_bearer_token`** and **`web_search_api_key`** are **secrecy `SecretString`** in [`AgentConfig`](DEVELOPMENT.md); **`Debug` / structured logs** avoid plaintext; use **`ExposeSecret::expose_secret()`** (re-exported from `config`). **`API_KEY`** is not part of `AgentConfig`.

## Environment variables (`AGENT_*`)

Common keys below; **full names and defaults** live in **`config/default_config.toml`**, **`config/session.toml`**, **`config/context_inject.toml`**, **`config/tools.toml`**, **`config/sandbox.toml`**, **`config/planning.toml`**, **`config/memory.toml`**. **`API_KEY`** is env-only (see “Model & API”); secret behavior under “Hot reload” above.

### Model & API

| Variable | Description |
| --- | --- |
| `API_KEY` | Cloud / OpenAI-compatible Bearer; with `llm_http_auth_mode=bearer` (default) sent as `Authorization` on `chat/completions` / `models`. **Not in TOML**; after change, re-export and **`/config reload`** or restart. With `none` (e.g. Ollama), omit. |
| `AGENT_API_BASE` | Overrides `api_base`. |
| `AGENT_MODEL` | Overrides `model`. |
| `AGENT_LLM_HTTP_AUTH_MODE` | `bearer` (needs **`API_KEY`**) or `none` (no `Authorization` on `chat/completions` / `models`). |
| `AGENT_LLM_REASONING_SPLIT` | Overrides `llm_reasoning_split`. If unset in TOML/env: **MiniMax** gateways (by `model` / `api_base`) default to **on**; others default **off** (see § MiniMax). |
| `AGENT_LLM_BIGMODEL_THINKING` | If true, Zhipu **`thinking: { "type": "enabled" }`** (GLM-5; see § GLM). |
| `AGENT_LLM_KIMI_THINKING_DISABLED` | If true, **`thinking: { "type": "disabled" }`** for Moonshot **kimi-k2.5** (see § Kimi). |
| `AGENT_SYSTEM_PROMPT` | Inline system prompt; clears inherited `system_prompt_file` unless `AGENT_SYSTEM_PROMPT_FILE` is set (see § System prompt). |
| `AGENT_SYSTEM_PROMPT_FILE` | Path to system prompt file. |
| `AGENT_DEFAULT_AGENT_ROLE` | Default **role id** when Web `agent_role` / CLI `--agent-role` is omitted (must exist in the role table; see § Multi-role). |

### Sampling

| Variable | Description |
| --- | --- |
| `AGENT_TEMPERATURE` | Overrides `temperature`. |
| `AGENT_LLM_SEED` | Overrides `llm_seed`. |

### Web server

| Variable | Description |
| --- | --- |
| `AGENT_HTTP_HOST` | Bind address when `--host` omitted. |
| `AGENT_WEB_API_BEARER_TOKEN` | Shared secret for protected Web APIs; send **`Authorization: Bearer …`** or **`X-API-Key: …`** (same value, pick one). |
| `AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK` | Allow unauthenticated non-loopback bind (**high risk**). |

### Workspace & Cursor-style rules

| Variable | Description |
| --- | --- |
| `AGENT_WORKSPACE_ALLOWED_ROOTS` | Comma-separated; same as `[agent] workspace_allowed_roots`. |
| `AGENT_CURSOR_RULES_ENABLED` | Enable rule file injection. |
| `AGENT_CURSOR_RULES_DIR` | Directory of `*.mdc`. |
| `AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD` | Append `AGENTS.md`. |
| `AGENT_CURSOR_RULES_MAX_CHARS` | Max injected chars. |

**Path safety (matches implementation)**: `workspace_allowed_roots` and per-request revalidation catch `..` escapes and symlinks that already point outside roots **at check time**. On **Unix**, **`read_file`** (`resolve_for_read_open`) and Web workspace list/read/write/delete go through **`src/workspace_fs.rs`**: on Linux, **`openat2` + `RESOLVE_IN_ROOT`** opens paths relative to an already-open workspace-root fd, narrowing the race between policy checks and `open`; symlinks inside the tree may still be followed, but resolution cannot escape the root. **Residual risk**: checks still depend on `canonicalize` at check time; non-Linux paths and code that does not use `workspace_fs` may still be TOCTOU-prone; **`create_dir_all`** + opens are not fully atomic. This is **not** a kernel sandbox; use **Web auth** on open networks. See **`src/path_workspace.rs`**.

### Planning & staged planning

| Variable | Description |
| --- | --- |
| `AGENT_FINAL_PLAN_REQUIREMENT` | `never` / `workflow_reflection` / `always`. |
| `AGENT_PLAN_REWRITE_MAX_ATTEMPTS` | Max plan rewrite rounds. |
| `AGENT_PLANNER_EXECUTOR_MODE` | `single_agent` / `logical_dual_agent`. |
| `AGENT_STAGED_PLAN_EXECUTION` | Enable staged planning. |
| `AGENT_STAGED_PLAN_PHASE_INSTRUCTION` | Planner phase instruction text. |
| `AGENT_STAGED_PLAN_ALLOW_NO_TASK` | Legacy; **no effect** (`no_task` rules come from embedded schema in the default planner system). |
| `AGENT_STAGED_PLAN_FEEDBACK_MODE` | `fail_fast` / `patch_planner`. |
| `AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS` | Max patch-planner rounds. |
| `AGENT_STAGED_PLAN_ENSEMBLE_COUNT` | Logical multi-planner count (1–3, default 1). |
| `AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM` | Print no-tools planner stream to stdout in CLI/`chat` (default `true`; see § Staged planning). |
| `AGENT_STAGED_PLAN_OPTIMIZER_ROUND` | Enable post-plan optimizer round (default `true`). |
| `AGENT_STAGED_PLAN_TWO_PHASE_NL_DISPLAY` | When `true`, suppress user-visible streaming for finalized no-tools plan JSON, then run a follow-up no-tools round for natural-language-only output (default `false`; see § Staged planning). |

### Queue, parallelism, cache

| Variable | Description |
| --- | --- |
| `AGENT_HEALTH_LLM_MODELS_PROBE` | When `1`/`true`, **`GET /health`** runs a **GET …/models** check (list endpoint only, no completion cost). Default off. |
| `AGENT_HEALTH_LLM_MODELS_PROBE_CACHE_SECS` | Cache probe results in-process (**5–86400**, default **120**) to limit upstream traffic from frequent health polls. |
| `AGENT_CHAT_QUEUE_MAX_CONCURRENT` | Max concurrent chat jobs. |
| `AGENT_CHAT_QUEUE_MAX_PENDING` | Max queued chat jobs. |
| `AGENT_PARALLEL_READONLY_TOOLS_MAX` | Max parallel readonly tools per round. |
| `AGENT_READ_FILE_TURN_CACHE_MAX_ENTRIES` | Per-turn `read_file` cache; `0` off; cleared on writes / workspace change. |
| `AGENT_TEST_RESULT_CACHE_ENABLED` | In-process test output LRU. |
| `AGENT_TEST_RESULT_CACHE_MAX_ENTRIES` | LRU size. Reuses truncated output for `cargo_test`, `rust_test_one`, `npm_run` (`script=test`), `run_command` `cargo`+`test` without `--nocapture` / `--test-threads`; first line **`[CrabMate test output cache hit]`**; not across restarts. |

### Session workspace changelist

| Variable | Description |
| --- | --- |
| `AGENT_SESSION_WORKSPACE_CHANGELIST_ENABLED` | Inject `crabmate_workspace_changelist` user message. |
| `AGENT_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS` | Max injected chars. Accumulates writes + unified diff per `long_term_memory_scope_id` (Web: `conversation_id`; CLI default `__default__`); not in session SQLite (stripped on save). **`workflow_execute` node tools** excluded. |

### Allowlist, MCP, conversation store

| Variable | Description |
| --- | --- |
| `AGENT_ALLOWED_COMMANDS` | Comma-separated `run_command` allowlist. Embedded defaults also include **`docker`**, **`podman`**, **`mvn`**, **`gradle`** (JVM/container built-ins and manual `run_command`); full list **`config/tools.toml`**. |
| `AGENT_MCP_ENABLED` | Enable MCP. |
| `AGENT_MCP_COMMAND` | MCP stdio launch command. |
| `AGENT_MCP_TOOL_TIMEOUT_SECS` | MCP tool timeout; one stdio session per fingerprint; **`crabmate mcp list`** needs no `API_KEY`; **`mcp list --probe`** spawns subprocess. |
| `AGENT_CODEBASE_SEMANTIC_SEARCH_ENABLED` | Register **`codebase_semantic_search`** (`false` removes from tool list). |
| `AGENT_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH` | Relative semantic index SQLite path; default **`.crabmate/codebase_semantic.sqlite`**. |
| `AGENT_CODEBASE_SEMANTIC_MAX_FILE_BYTES` | Max bytes per indexed file. |
| `AGENT_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS` | Max chars per chunk. |
| `AGENT_CODEBASE_SEMANTIC_TOP_K` | Default Top-K. |
| `AGENT_CODEBASE_SEMANTIC_REBUILD_MAX_FILES` | Max files **re-embedded** per **`rebuild_index`** (large-repo guard; unchanged files are skipped in incremental mode). |
| `AGENT_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL` | Workspace-wide **`rebuild_index`** defaults to **incremental** (**`mtime` + `size` + SHA256**); **`false`** clears chunk + file-catalog rows then full re-embed. Subtree **`path`** still replaces that prefix only. |
| `AGENT_CONVERSATION_STORE_SQLITE_PATH` | Conversation SQLite path. |

### First-turn injection

| Variable | Description |
| --- | --- |
| `AGENT_MEMORY_FILE_ENABLED` | Workspace memo file injection. |
| `AGENT_MEMORY_FILE` | Memo path. |
| `AGENT_MEMORY_FILE_MAX_CHARS` | Memo max chars. |
| `AGENT_PROJECT_PROFILE_INJECT_ENABLED` | Project profile injection. |
| `AGENT_PROJECT_PROFILE_INJECT_MAX_CHARS` | Profile max chars. |
| `AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED` | Dependency brief (merged with profile/memo). |
| `AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS` | From `cargo metadata` edges + Mermaid + root/`frontend` `package.json` name excerpts; `0` disables segment. |

### Tool explain card

| Variable | Description |
| --- | --- |
| `AGENT_TOOL_CALL_EXPLAIN_ENABLED` | Require `crabmate_explain_why` on mutating tools. |
| `AGENT_TOOL_CALL_EXPLAIN_MIN_CHARS` | Min explain length. |
| `AGENT_TOOL_CALL_EXPLAIN_MAX_CHARS` | Max explain length. |

### Long-term memory

| Variable | Description |
| --- | --- |
| `AGENT_LONG_TERM_MEMORY_ENABLED` | Enable long-term memory. |
| `AGENT_LONG_TERM_MEMORY_SCOPE_MODE` | Scope mode. |
| `AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND` | Default `fastembed` or `disabled`. |
| `AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH` | SQLite for vectors/metadata. |
| `AGENT_LONG_TERM_MEMORY_TOP_K` | Retrieval Top-K. |
| `AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK` | Max chars per chunk. |
| `AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX` | Min chars to index. |
| `AGENT_LONG_TERM_MEMORY_ASYNC_INDEX` | Async indexing. |
| `AGENT_LONG_TERM_MEMORY_MAX_ENTRIES` | Max entries. |
| `AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS` | Max chars injected into model context. |

With Web `conversation_store_sqlite_path`, session and memory may share one SQLite; pure in-memory sessions need **`long_term_memory_store_sqlite_path`** for persistence. CLI default: `run_command_working_dir/.crabmate/long_term_memory.db`. If enabled but DB open fails: one **stderr** warning, process continues without injection.

### Web search & `http_fetch`

| Variable | Description |
| --- | --- |
| `AGENT_WEB_SEARCH_PROVIDER` | Provider id. |
| `AGENT_WEB_SEARCH_API_KEY` | Search API key. |
| `AGENT_WEB_SEARCH_TIMEOUT_SECS` | Timeout seconds. |
| `AGENT_WEB_SEARCH_MAX_RESULTS` | Max results. |
| `AGENT_HTTP_FETCH_ALLOWED_PREFIXES` | Allowed URL prefixes. |
| `AGENT_HTTP_FETCH_TIMEOUT_SECS` | Fetch timeout. |
| `AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES` | Max response bytes. |

**Outer `tokio::time::timeout` around `spawn_blocking`**: besides **`http_fetch_timeout_secs`** (client read timeout), the async path wraps blocking work. Defaults align with **`command_timeout_secs`** and **`http_fetch_timeout_secs`**. Override with TOML **`[tool_registry]`** keys **`http_fetch_wall_timeout_secs`** / **`http_request_wall_timeout_secs`** (see commented examples at the end of **`config/tools.toml`**).

### `tool_registry` policy (`tools.toml` / main config)

Optional table **`[tool_registry]`** in **`config/tools.toml`** or your **`config.toml`** (merged like other fragments) maps into **`AgentConfig`** and is updated on hot reload. **No `AGENT_*` aliases**—use TOML.

| Key | Purpose |
| --- | --- |
| **`http_fetch_wall_timeout_secs`** | Outer timeout for **`http_fetch`** (seconds). |
| **`http_request_wall_timeout_secs`** | Outer timeout for **`http_request`**; if omitted, follows fetch outer logic. |
| **`parallel_wall_timeout_secs`** | Subtable: per-**`ToolExecutionClass`** snake_case keys (**`blocking_sync`**, **`http_fetch_spawn_timeout`**, …) overriding parallel readonly batch / **`SyncDefault`+`spawn_blocking`** wall clocks. |
| **`parallel_sync_denied_tools`** | Tool names never batched with other readonly tools (exact match); default built-in denylist if omitted. |
| **`parallel_sync_denied_prefixes`** | Same, by name prefix. |
| **`sync_default_inline_tools`** | **`SyncDefault`** tools run inline on the async task (skip **`spawn_blocking`**); default small builtin set if omitted. |
| **`write_effect_tools`** | Tools treated as mutating for **`is_readonly_tool`**, explain card, codebase semantic invalidation, etc.; default builtin set if omitted. |

### Context & tool messages

| Variable | Description |
| --- | --- |
| `AGENT_MAX_MESSAGE_HISTORY` | Max messages kept. |
| `AGENT_TOOL_MESSAGE_MAX_CHARS` | Compress `role: tool` before model if longer. |
| `AGENT_TOOL_RESULT_ENVELOPE_V1` | `crabmate_tool` envelope v1. |
| `AGENT_TOOL_STATS_ENABLED` | When truthy, enable in-process tool-outcome stats and append a short hint to the **new** conversation’s first `system` (see below). |
| `AGENT_TOOL_STATS_WINDOW_EVENTS` | Sliding-window event cap (16–65536); mirrors TOML `agent_tool_stats_window_events`. |
| `AGENT_TOOL_STATS_MIN_SAMPLES` | Min total calls per tool in the window before it appears in the hint (1–10000). |
| `AGENT_TOOL_STATS_MAX_CHARS` | Max Unicode scalars for the appendix (64–32768; truncated if longer). |
| `AGENT_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO` | Hint if success rate is below this (0.0–1.0) and `min_samples` is met; failures always qualify. |
| `AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS` | Materialize DeepSeek DSML tool calls. |
| `AGENT_THINKING_AVOID_ECHO_SYSTEM_PROMPT` | Append the thinking-discipline appendix to the first `system` message; defaults to on. |
| `AGENT_THINKING_AVOID_ECHO_APPENDIX` | Inline appendix body (non-empty clears the file path; if **`…_FILE`** is set afterward, **file wins**). |
| `AGENT_THINKING_AVOID_ECHO_APPENDIX_FILE` | Path to appendix Markdown (same resolution as **`system_prompt_file`**). |
| `AGENT_CONTEXT_CHAR_BUDGET` | Character budget trim. |
| `AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM` | Min messages after system post-summary. |
| `AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS` | Trigger summary when over char threshold. |
| `AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES` | Tail messages kept after summary. |
| `AGENT_CONTEXT_SUMMARY_MAX_TOKENS` | Summary request max_tokens. |
| `AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS` | Summary transcript max chars. |

**`[agent]` TOML keys (tool stats)**: `agent_tool_stats_enabled`, `agent_tool_stats_window_events`, `agent_tool_stats_min_samples`, `agent_tool_stats_max_chars`, `agent_tool_stats_warn_below_success_ratio`. Stats are **per-process**, **global** (not bucketed by `conversation_id`); **no** tool args or full outputs stored. Web attaches the stats appendix only for **new** chats (no stored seed). CLI `chat` / `repl` and `workspace_session::initial_workspace_messages` attach on fresh first-`system` paths; sessions loaded from disk keep base system alignment **without** the stats appendix.

### CLI

| Variable | Description |
| --- | --- |
| `AGENT_TUI_LOAD_SESSION_ON_START` | Load session from disk on start. |
| `AGENT_TUI_SESSION_MAX_MESSAGES` | Max messages in session file. |
| `AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED` | If `true`, background `initial_workspace_messages` (profile, deps); default `false`. TOML: `[agent] repl_initial_workspace_messages_enabled`. |
| `AGENT_CLI_WAIT_SPINNER` | If truthy, show indicatif spinner on stderr before first stream chunk in CLI/`chat` (needs TTY stderr, not **`NO_COLOR`**). |

### Docker tool sandbox

| Variable | Description |
| --- | --- |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE` | `none` \| `docker`. |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE` | Required image in `docker` mode. |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK` | Empty = no network; `bridge` for outbound tools. |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS` | Per-container wait cap. |
| `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER` | Docker `Config.user`; `current`/`host` semantics in § SyncDefault Docker below. |

You may also use **`DOCKER_HOST`** (non-`AGENT_`) like the `docker` CLI / bollard.

```bash
export AGENT_MODEL=deepseek-reasoner
cargo run
```

## Local Ollama (OpenAI-compatible)

Ollama serves OpenAI-compatible API at **`http://127.0.0.1:11434/v1`**. Example:

```toml
[agent]
api_base = "http://127.0.0.1:11434/v1"
model = "llama3.2"   # use `ollama list`
llm_http_auth_mode = "none"
```

Then **`API_KEY`** is not required for `serve` / `repl` / `chat`. Function-calling quality depends on model/Ollama; try **`--no-tools`** to validate chat. `crabmate config` does **not** need **`API_KEY`**.

## MiniMax (OpenAI-compatible)

MiniMax **`https://api.minimaxi.com/v1`** (aliases like **`https://api.minimax.io/v1`** may exist—use console). Docs show **`role: "system"`** but live API often returns **`invalid message role: system`**. CrabMate **auto-merges** **`system`** into **`user`** when **`model` / `api_base`** identify MiniMax (no TOML key). Other gateways keep a standalone **`system`** message.

Tested model IDs in this repo: **`MiniMax-M2.7`**, **`MiniMax-M2.7-highspeed`**, **`MiniMax-M2.5`**.

```toml
[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"
llm_http_auth_mode = "bearer"
# llm_reasoning_split: omit → defaults to true on MiniMax; set false to disable
```

**`API_KEY`** as Bearer. When **`llm_reasoning_split`** is true (including MiniMax default when omitted), the request includes **`reasoning_split: true`**; streaming **`delta.reasoning_details`** may fold into **`reasoning_content`**.

### Less system-prompt echo in thinking/reasoning

Default **`thinking_avoid_echo_system_prompt = true`** (**`[agent]`**, embedded default in **`config/default_config.toml`**, same section as **`system_prompt_file`**). Appendix text defaults from **`thinking_avoid_echo_appendix_file`** (shipped **`config/prompts/thinking_avoid_echo_appendix.md`** — edit on disk without rebuilding); optional **`thinking_avoid_echo_appendix`** inline string. **Precedence**: non-empty **`thinking_avoid_echo_appendix_file`** is read from disk **before** inline; if neither is set, a compile-time embedded default is used. **`tool_stats::augment_system_prompt`** appends the resolved body to the **first `system`** of **new** Web/CLI chats. **Soft** hint only. Disable with **`thinking_avoid_echo_system_prompt = false`** or **`AGENT_THINKING_AVOID_ECHO_SYSTEM_PROMPT=0`**.

## Zhipu GLM (OpenAI-compatible)

**`api_base`**: **`https://open.bigmodel.cn/api/paas/v4`** (do not append `/chat/completions`). **`model`**: e.g. **`glm-5`**. **`API_KEY`** as Bearer.

Minimal vendor-style request: **`model`**, **`messages`**, **`stream: true`** without **`thinking`**. CrabMate with **`llm_bigmodel_thinking = false`** omits **`thinking`**; Web/CLI streaming uses **`stream: true`**.

Optional deep thinking: **`llm_bigmodel_thinking = true`** (**`AGENT_LLM_BIGMODEL_THINKING=1`**) → **`thinking: { "type": "enabled" }`** per [GLM-5 docs](https://docs.bigmodel.cn/cn/guide/models/text/glm-5).

## Moonshot Kimi (OpenAI-compatible)

**`POST https://api.moonshot.cn/v1/chat/completions`**. In CrabMate: **`api_base` = `https://api.moonshot.cn/v1`**. Models: **`kimi-k2.5`**, **`kimi-k2-thinking`**, **`moonshot-v1-8k`**, etc.—see [Kimi docs](https://platform.moonshot.cn/docs/api/chat).

**`max_tokens` vs `max_completion_tokens`**: Kimi deprecates **`max_tokens`** in favor of **`max_completion_tokens`**; CrabMate still sends **`max_tokens`** from **`[agent]`** for compatibility—if you hit length-related 400s, lower **`max_tokens`** or watch for future **`max_completion_tokens`** support.

**`thinking` (kimi-k2.5 only)**: Optional **`enabled`/`disabled`**; server default near enabled. **`llm_kimi_thinking_disabled = true`** sends **`thinking: { "type": "disabled" }`** only when **`model`** matches **`kimi-k2.5*`**. If both **`llm_bigmodel_thinking`** and Kimi apply, **Kimi disabled wins**.

**Multi-turn + tools**: With k2.5 default thinking, assistants with **`tool_calls`** may need **`reasoning_content`**; CrabMate preserves or pads empty **`reasoning_content`** on those messages when required.

**`temperature`**: Auto-clamped: **`kimi-k2.5*`** and **`kimi-k2-thinking*`** → **1.0**; other **`kimi-k2*`** (e.g. **`kimi-k2-0905-preview`**) → **0.6**; **`moonshot-v1-*`** uses configured **`temperature`**.

```toml
[agent]
api_base = "https://api.moonshot.cn/v1"
model = "kimi-k2.5"
llm_http_auth_mode = "bearer"
# llm_kimi_thinking_disabled = true   # optional: disable k2.5 default thinking
```

## Sample `config.toml`

```toml
[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-reasoner"
# system_prompt = "…"
# system_prompt_file = "my_prompt.txt"
# cursor_rules_enabled = true
# cursor_rules_dir = ".cursor/rules"
```

## Final answer plan (`final_plan_requirement`)

When the model ends a turn **without** `tool_calls`, whether an embeddable **`agent_reply_plan`** JSON is required (details: **[DEVELOPMENT.md](DEVELOPMENT.md)**).

- **`workflow_reflection`** (default): require plan only after workflow reflection path.
- **`never`**: no enforcement.
- **`always`** (experimental): every final answer—**higher cost**.

With `workflow_validate_only` results, **`spec.layer_count`** constrains step count. Optional **`workflow_node_id`** must be a subset of **`nodes[].id`** from the latest **`workflow_execute`** result.

**Strict node coverage (`final_plan_require_strict_workflow_node_coverage`, default `false`, `AGENT_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE`)**: when `true`, if **any** step sets `workflow_node_id`, the plan must reference **every** `nodes[].id` from the latest workflow tool result at least once. If no step sets `workflow_node_id`, this rule does not apply.

**Optional semantic side-check LLM (default off)**: **`final_plan_semantic_check_enabled`** (`AGENT_FINAL_PLAN_SEMANTIC_CHECK_ENABLED`, default `false`) with **`final_plan_requirement = workflow_reflection`**: after static checks pass, if a tool digest can be built from history, one extra no-tools `chat/completions` asks whether the plan contradicts recent tool output; **`INCONSISTENT`** triggers the same rewrite path as other failures (counts against **`plan_rewrite_max_attempts`**). **`final_plan_semantic_check_max_non_readonly_tools`** (`AGENT_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS`, default `0`, range 0–32) caps extra non-readonly tool lines in the digest; at `0`, high-risk builtin names (e.g. `run_command`, `workflow_execute`) and readonly tools may still appear. **`final_plan_semantic_check_max_tokens`** (`AGENT_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS`, default `256`, clamp 32–1024) sets side-call `max_tokens`. Parse/API failures **fail open** (treat as consistent).

## Plan rewrite (`plan_rewrite_max_attempts`)

Max “please rewrite” user injections when the plan is invalid; when exhausted, stream may emit **`code: plan_rewrite_exhausted`** (optional sibling **`reason_code`**, see **`docs/en/SSE_PROTOCOL.md`**).

## Logical dual agent (`planner_executor_mode = logical_dual_agent`)

No-tools planning round first, then executor loop; planner context strips `role: tool` bodies. Takes precedence over **`staged_plan_execution`** when both apply.

## Staged planning (`staged_plan_execution`)

With **`planner_executor_mode = single_agent`**, each user message runs a no-tools plan round then **`steps`**. **`no_task` + empty `steps`** skips execution. Invalid plan JSON falls back to normal tool loop (more API calls than off).

**`staged_plan_feedback_mode`**: Default **`fail_fast`**; **`patch_planner`** injects feedback and reruns planner without tools, merging patched **`steps`** (capped by **`staged_plan_patch_max_attempts`**).

**`staged_plan_cli_show_planner_stream`** (default `true`, **`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`**)**: For CLI/`chat` with **`out: None`**, whether no-tools planner (and patch planner) streams to stdout. **`false`** hides planner raw output but keeps notices and execution steps; Web SSE unchanged.

**`staged_plan_optimizer_round`** (default `true`): After first plan with ≥2 steps, optional no-tools round to merge read-only probes and parallelize per **`parallel_readonly_tools`** rules.

**`staged_plan_optimizer_requires_parallel_tools`** (default `true`, **`AGENT_STAGED_PLAN_OPTIMIZER_REQUIRES_PARALLEL_TOOLS`**)**: When `true`, skip the optimizer round if this turn’s tool list has **no** built-in names eligible for same-turn parallel readonly batching (the optimizer prompt centers on that CSV). When `false`, keep the legacy behavior: run the optimizer whenever step count and **`staged_plan_optimizer_round`** allow it.

**`staged_plan_ensemble_count`** (default `1`, clamp 1–3, **`AGENT_STAGED_PLAN_ENSEMBLE_COUNT`**)**: **`1`** off. **`2`/`3`**: extra serial no-tools “planner B/C” rounds (aux assistants **not** in history), then merge round—**significantly more API cost**.

**`staged_plan_skip_ensemble_on_casual_prompt`** (default `true`, **`AGENT_STAGED_PLAN_SKIP_ENSEMBLE_ON_CASUAL_PROMPT`**)**: When **`staged_plan_ensemble_count` > 1**, skip ensemble + merge if the **current user message** (heuristic: very short or common small-talk) looks casual—saves planner API calls. Set `false` to always run ensemble when configured.

**Two-phase display (`staged_plan_two_phase_nl_display`, default `false`, `AGENT_STAGED_PLAN_TWO_PHASE_NL_DISPLAY`)**: When `true`, after a parsed **`agent_reply_plan` v1** is merged into history (including optional ensemble/merge + optimizer; **`no_task`** path also runs this before the regular loop), **no-tools planner-class rounds** call **`complete_chat_retrying`** with **no user-visible streaming** of the plan JSON (`out: None` and suppressed `render_to_terminal`, combined with **`staged_plan_cli_show_planner_stream`** for CLI). A bridging **user** (`staged_plan_nl_followup_user_body`: short colloquial follow-up plus the same kind of display-hidden prefix as staged step injections; **not** shown in chat, reducing “the user sent system instructions” narration) is appended, then another **no-tools** completion streams **natural language only**. History keeps JSON assistant + bridge user + NL assistant. There is **no** vendor **`response_format: json_object`** enforcement; the first round still relies on fence/body parsing. **`patch_planner`** replans mid-run **do not** automatically trigger this NL follow-up (only the initial finalize path does).

## SyncDefault Docker sandbox (`sync_default_tool_sandbox_mode`)

### Modes

- **`none` (default)**: **`SyncDefault`** and **`run_command`** run on host **`spawn_blocking`**.
- **`docker`**: After allowlist/approval on host, **SyncDefault**, **`run_command`**, **`run_executable`**, **`get_weather`**, **`web_search`**, **`http_fetch`**, **`http_request`** run in ephemeral containers via **bollard** (like `docker run --rm -i`): workspace at **`/workspace`**, read-only host **`crabmate`** at **`/crabmate`**, internal **`crabmate tool-runner-internal`**. **`workflow_execute`** and **`mcp__*`** stay on host.

**bollard crate features (maintainers)**: Root **`Cargo.toml`** sets **`default-features = false`** on **bollard** and enables only **`http`** + **`pipe`** (local **`unix://`**, Windows named pipes, plain **`tcp://`** / **`http://`** **`DOCKER_HOST`**—smaller deps/binary). For **`https://`** **`DOCKER_HOST`** or **`DOCKER_TLS_VERIFY`**, add **`ssl`** to **bollard**’s **`features`** and rebuild (pulls **rustls**, etc.).

### Prerequisites

1. Docker daemon reachable (`docker ps` or **`DOCKER_HOST`**).
2. **Same CPU arch** as host `crabmate` binary (mounted into container).
3. **Image** supplies CLIs you use (`git`, `rg`, `cargo`, …); repo ships no fixed “official tools image”—`config/sandbox.toml` placeholder only.

### Minimal Dockerfile

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git ripgrep curl \
  && rm -rf /var/lib/apt/lists/*
```

### Enable

```toml
[agent]
sync_default_tool_sandbox_mode = "docker"
sync_default_tool_sandbox_docker_image = "your-registry/crabmate-tools:dev"
# sync_default_tool_sandbox_docker_network = "bridge"
# sync_default_tool_sandbox_docker_timeout_secs = 600
# sync_default_tool_sandbox_docker_user = "current"
```

Env: `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE`, `AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`, etc.

### Network

- **Empty network**: **`none`**—no egress (local tools only).
- **`bridge`** (etc.): outbound for weather/search/http tools—use carefully on untrusted workspaces.

### Timeout & user

- **`sync_default_tool_sandbox_docker_timeout_secs`**: Container wait cap (default 600s), then force remove.
- **`sync_default_tool_sandbox_docker_user`**: Docker **`user`**. Default **`current`/`host`**: Unix **euid:egid**; **`image`/`default`**: image **`USER`**; other values passed through.

### Security & ops

- Runner JSON in **`TMPDIR`** (mode **`0600`** when possible) may include **`web_search_api_key`**—trusted hosts only.
- Sandbox **does not replace** allowlist, HTTP prefix rules, or Web/CLI approval.
- Per-invocation container start/stop adds latency vs **`none`**.

## System prompt

- **Default**: **`system_prompt_file = "config/prompts/default_system_prompt.md`** (read at runtime; edit without rebuild).
- **Relative path resolution**: process **cwd** → each overlay **config file directory** (later wins, e.g. `.agent_demo.toml` before `config.toml`) → **`run_command_working_dir`**. **Absolute** paths tried as-is.
- **Overrides**: Inline **`system_prompt`** without **`system_prompt_file`** in a layer **drops** inherited file for that layer. Env: **`AGENT_SYSTEM_PROMPT`** clears merged file; **`AGENT_SYSTEM_PROMPT_FILE`** wins if both set.
- **finalize**: Read file if **`system_prompt_file`** set; else non-empty inline; else error.

## Multi-role (agent_roles)

Besides the global `system_prompt`, you can define **named ids** with their own first-turn `system` text (each merged with **`cursor_rules_*`** like the global prompt).

- **Sources** (later overlays win for the same id):  
  1. **`[[agent_roles]]`** rows in the main config: **`id`**, plus **`system_prompt`** and/or **`system_prompt_file`**. Empty inline **`system_prompt`** means **inherit** the global merged system.  
  2. **`config/agent_roles.toml`** when not using **`--config`**; with **`--config path/to/foo.toml`**, read **`path/to/agent_roles.toml`** next to it. Shape: **`[agent_roles]`**, optional **`default_role`**, **`[agent_roles.roles.<id>]`** (see `config/agent_roles.toml`).
- **Default role**: **`[agent] default_agent_role`**, or **`agent_roles.toml` `[agent_roles] default_role`**, or **`AGENT_DEFAULT_AGENT_ROLE`**. Must reference a defined id; if unset, omitting `agent_role` uses the global **`system_prompt`**.
- **Web**: optional JSON **`agent_role`** on **`POST /chat`** / **`POST /chat/stream`**. Applied only when the server **has no stored history** for that **`conversation_id`** (first turn); ignored for existing sessions.
- **CLI**: global **`--agent-role <id>`** for **`repl`** / **`chat`**. Mutually exclusive with **`chat --system-prompt-file`**. For **`chat`** without **`--messages-json-file`**, applies to the first-turn system (including **`--message-file`** first line).
- **REPL**: **`/agent list`** prints the built-in pseudo id **`default`** first (no explicit named role; same as Web omitting **`agent_role`**: **`default_agent_role_id`** if set, else global **`system_prompt`**), then configured ids (same names as **`GET /status`** **`agent_role_ids`**). **`/agent set default`** (case-insensitive) clears the explicit REPL role and rebuilds the first-turn system messages.
- **Hot reload**: role table reloads with **`POST /config/reload`** / **`/config reload`**.
- **`GET /status`**: **`agent_role_ids`**, **`default_agent_role_id`**.

## Cursor-like rules

When **`cursor_rules_enabled`**, append sorted **`cursor_rules_dir`/*.mdc** (optional **`AGENTS.md`**) to system prompt, capped by **`cursor_rules_max_chars`**.

## Context window

Before each model call: trim by count, **`context_char_budget`**, optional LLM summary. **`tool_message_max_chars`**: compress long **`role: tool`**; with **`tool_result_envelope_v1`**, head/tail sample **`crabmate_tool.output`** (see **[DEVELOPMENT.md](DEVELOPMENT.md)**). Details: **`config/tools.toml`**.

## Web chat queue (`chat_queue_*`)

`/chat` and `/chat/stream` use a bounded queue; full → **503** **`QUEUE_FULL`**. **`/status`** exposes queue and **`per_active_jobs`**.

## Readonly tool parallelism (`parallel_readonly_tools_max`)

Caps concurrent readonly tools: eligible batch includes **`SyncDefault`**, **`http_fetch`** (GET/HEAD), **`get_weather`**, **`web_search`** (not **`http_request`**, **`run_command`**, MCP). Build-lock tools (**`cargo_*`**, **`npm_*`**) force serial batch.

## HTTP client

Single process-wide **`reqwest::Client`** (pool, keep-alive). See **`http_client`** in **[DEVELOPMENT.md](DEVELOPMENT.md)**.

## Common model IDs

- `deepseek-chat` (default)
- `deepseek-reasoner` (longer reasoning)
