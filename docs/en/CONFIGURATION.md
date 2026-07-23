**Languages / 语言:** [中文](../配置说明.md) · English (this page)

# Configuration

Default settings are merged from seven embedded TOML fragments under **`config/`**: **`default_config.toml`**, **`session.toml`**, **`context_inject.toml`**, **`tools.toml`**, **`sandbox.toml`**, **`planning.toml`**, **`memory.toml`** (each fragment is mostly flattened under **`[agent]`**; **`config/tools.toml`** may also define optional **`[tool_registry]`**—see “`tool_registry` policy” below). **`session`** covers CLI session **`tui_*`** and **`repl_initial_workspace_messages_enabled`**; **`context_inject`** covers first-turn **`living_docs_*`**, **`agent_memory_file_*`**, **`project_profile_inject_*`**, **`project_dependency_brief_inject_*`**; **`tools`** **`[agent]`** covers **`run_command`** allowlist/timeouts/working dir, **`tool_message_*`** / **`tool_result_envelope_v1`**, **`read_file_turn_cache_*`**, **`test_result_cache_*`**, **`session_workspace_changelist_*`**, **`codebase_semantic_*`** (the **`codebase_semantic_search`** tool), weather/search/**`http_fetch_*`**, **`tool_call_explain_*`**, **`mcp_*`**, etc.; **`sandbox`** is **SyncDefault Docker** **`sync_default_tool_sandbox_*`**; **`planning`** is planning/reflection/orchestration; **`memory`** is **`long_term_memory_*`**. `load_config` merges in order **defaults → session → context_inject → tools → sandbox → planning → memory**, then **`config.toml`** or **`.agent_demo.toml`**, then environment variables.

## Hot reload (without restarting `repl` / `serve`)

- **CLI**: Type **`/config reload`** (Tab completes). Re-reads the same config path as startup (**`--config`** or default **`config.toml`** / **`.agent_demo.toml`**), merges with **current process env**, writes hot fields into in-memory [`AgentConfig`](DEVELOPMENT.md); clears MCP stdio cache; next turn uses the new MCP fingerprint.
- **Web**: **`POST /config/reload`** (JSON body may be `{}`; same auth as **`/chat`** and other protected APIs—**`Authorization: Bearer <token>`** or **`X-API-Key: <token>`** when the layer is enabled). Success: **`{ "ok": true, "message": "…" }`**.
- **Typically hot-reloaded**: **`api_base`**, **`model`**, **`llm_http_auth_mode`**, **`llm_reasoning_split`**, **`llm_bigmodel_thinking`**, **`llm_kimi_thinking_disabled`**, **`thinking_avoid_echo_system_prompt`**, **`thinking_avoid_echo_appendix` / `thinking_avoid_echo_appendix_file`** (resolved appendix text), **`temperature` / `llm_seed`**, timeouts/retries, **`run_command`** allowlist, **`http_fetch_allowed_prefixes`**, **`workspace_allowed_roots`**, **`web_api_bearer_token`** (handler-side check only; see below), **`web_audit_log_write_tools`**, **`web_audit_trust_x_forwarded_for`** (write-tool audit and optional **`X-Forwarded-For`** trust), **`mcp_*`**, **`[tool_registry]`** fields (outer HTTP walls, parallel wall overrides, deny/inline/write-effect lists), **`system_prompt_file` re-read**, context/planning keys (implementation: **`apply_hot_reload_config_subset`**). **`system`→`user` folding** for MiniMax follows **`model` / `api_base`** on the next request after reload (not an `AgentConfig` field).
- **Not hot-reloaded**: **`conversation_store_sqlite_path`** (SQLite opened at startup—change path requires **`serve` restart**). **`reqwest::Client`** is not rebuilt; **`api_timeout_secs`** may lag on pooled idle connections.
- **`API_KEY`**: Still **environment only**; hot reload does not read secret files. After changing **`API_KEY`**, re-**export** and **`/config reload`** (or restart) for **`llm_http_auth_mode=bearer`** consistency.
- **Web API auth layer**: Embedded default **`web_api_require_bearer=false`**: **`serve`** may start without **`web_api_bearer_token`** / **`CM_WEB_API_BEARER_TOKEN`**. After a successful start, if the token is non-empty, the auth middleware is mounted for the process lifetime; clients send **`Authorization: Bearer <same secret>`** or **`X-API-Key: <same secret>`** (either). Set **`web_api_require_bearer=true`** (or **`CM_WEB_API_REQUIRE_BEARER=1`**) to **refuse startup** until a non-empty shared secret is configured. Hot reload **does not** add/remove the layer—switching between “no token” and “token” requires **`serve` restart**. Hot reload still updates the secret string used inside handlers when the layer exists. For a **personal VPS + TLS reverse proxy** walkthrough (Chinese), see **`docs/个人VPS部署指南.md`**.
- **Write-tool audit (structured logs)**: When **`web_audit_log_write_tools`** defaults **on**, each successful **non-readonly** built-in tool emits one **`info`** line with **`target=crabmate::audit_write_tool`** (timestamp ms, `job_id`, scope id, **`http`** vs **`scheduled`**, `client_ip` / `peer_ip`, **`bearer_fp`** as the first 12 hex chars of SHA-256 of the shared secret when the request’s **`Authorization` / `X-API-Key`** matches, otherwise **`-`**, tool name, redacted **`args_preview`**). **No** raw secrets in log text. Non-Web entrypoints (CLI, bench) do not emit these lines. **`web_audit_trust_x_forwarded_for`** (default **off**): when **on**, `client_ip` prefers the first hop in **`X-Forwarded-For`**; enable only behind a **trusted** reverse proxy.
- **Secrets in memory**: **`web_api_bearer_token`** and **`web_search_api_key`** are **secrecy `SecretString`** in [`AgentConfig`](DEVELOPMENT.md); **`Debug` / structured logs** avoid plaintext; use **`ExposeSecret::expose_secret()`** (re-exported from `config`). **`API_KEY`** is not part of `AgentConfig`.

## Environment variables (`CM_*`)

Common keys below; **full names and defaults** live in **`config/default_config.toml`**, **`config/session.toml`**, **`config/context_inject.toml`**, **`config/tools.toml`**, **`config/sandbox.toml`**, **`config/planning.toml`**, **`config/memory.toml`**. **`API_KEY`** is env-only (see “Model & API”); secret behavior under “Hot reload” above.

### Model & API

| Variable | Description |
| --- | --- |
| `API_KEY` | Cloud / OpenAI-compatible Bearer; with `llm_http_auth_mode=bearer` (default) sent as `Authorization` on `chat/completions` / `models`. **Not in TOML**; after change, re-export and **`/config reload`** or restart. With `none` (e.g. Ollama), omit. |
| `CM_API_BASE` | Overrides `api_base`. |
| `CM_MODEL` | Overrides `model`. |
| `CM_LLM_HTTP_AUTH_MODE` | `bearer` (needs **`API_KEY`**) or `none` (no `Authorization` on `chat/completions` / `models`). |
| `CM_LLM_REASONING_SPLIT` | Overrides `llm_reasoning_split`. If unset in TOML/env: **MiniMax** gateways (by `model` / `api_base`) default to **on**; others default **off** (see § MiniMax). |
| `CM_LLM_BIGMODEL_THINKING` | If true, Zhipu **`thinking: { "type": "enabled" }`** (GLM-5; see § GLM). |
| `CM_LLM_KIMI_THINKING_DISABLED` | If true, **`thinking: { "type": "disabled" }`** for Moonshot **kimi-k2.5** (see § Kimi). |
| `CM_SYSTEM_PROMPT` | Inline system prompt; clears inherited `system_prompt_file` unless `CM_SYSTEM_PROMPT_FILE` is set (see § System prompt). |
| `CM_SYSTEM_PROMPT_FILE` | Path to system prompt file. |
| `CM_DEFAULT_CM_ROLE` | Default **role id** when Web `agent_role` / CLI `--agent-role` is omitted (must exist in the role table; see § Multi-role). |

### Sampling

| Variable | Description |
| --- | --- |
| `CM_TEMPERATURE` | Overrides `temperature`. |
| `CM_LLM_SEED` | Overrides `llm_seed`. |

### Web server

| Variable | Description |
| --- | --- |
| `CM_HTTP_HOST` | Bind address when `--host` omitted. |
| `CM_WEB_API_BEARER_TOKEN` | Shared secret for protected Web APIs; send **`Authorization: Bearer …`** or **`X-API-Key: …`** (same value, pick one). |
| `CM_WEB_API_REQUIRE_BEARER` | If unset, inherits embedded default (**`false`**): **`serve`** may start without **`CM_WEB_API_BEARER_TOKEN`** / TOML **`web_api_bearer_token`**; set **`1`/`true`** to require a non-empty secret at startup (same as **`[agent] web_api_require_bearer=true`**). |
| `CM_WEB_AUDIT_LOG_WRITE_TOOLS` | Overrides **`web_audit_log_write_tools`**; default **on**—structured audit for write-side-effect tools (**`target=crabmate::audit_write_tool`**). |
| `CM_WEB_AUDIT_TRUST_X_FORWARDED_FOR` | Overrides **`web_audit_trust_x_forwarded_for`**; default **off**—whether audit **`client_ip`** trusts the first **`X-Forwarded-For`** hop. |
| `CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK` | Allow unauthenticated non-loopback bind (**high risk**). |
| `CM_WEB_STATIC_DIR` | Override **`serve`** static root (must contain **`index.html`**, **`vendor/ide-codemirror.js`**, etc.). **No** TOML key; read when the sidecar/**`serve`** process starts—restart after change. Default resolution: **`crates/crabmate-internal/src/web_static_dir.rs`** (dev: usually repo **`frontend/dist`**; desktop **`.deb`**: **`/usr/share/crabmate/frontend/dist`**, auto-injected by Tauri when **not** running from a source tree). **`cargo tauri dev`** sets this to repo **`frontend/dist`** when built, else clears inherited **`/usr/share/…`**. If the env still points at the install path but **`serve` cwd** is a built source tree, resolution **prefers local dist** (dev machine with deb installed). |
| `CM_DESKTOP_WORKDIR` | **Desktop Tauri sidecar only**: overrides **`crabmate serve`** **`current_dir`**. **No** TOML key. Default: repo root in source-tree dev; writable **`$HOME`** for **`.deb`** (relative **`.crabmate/conversations.db`** resolves under this cwd). **Do not** point at read-only **`/usr/share/crabmate`**; static UI is **`CM_WEB_STATIC_DIR`**. See **`desktop-tauri/DEVELOPMENT.md`**, **`docs/design/user_data_dir.md`**. |

### Desktop Tauri (sidecar)

The shell spawns **`crabmate serve --desktop-ready-json`**. Besides **`CM_DESKTOP_WORKDIR`** / **`CM_WEB_STATIC_DIR`**, use **`CM_DESKTOP_BACKEND_BIN`** in dev (see **`desktop-tauri/README.md`**). Before packaging: **`cd frontend && trunk build --release`** and **`bash desktop-tauri/scripts/prepare-sidecar.sh`**.

### Workspace & Cursor-style rules

| Variable | Description |
| --- | --- |
| `CM_WORKSPACE_ALLOWED_ROOTS` | Comma-separated; same as `[agent] workspace_allowed_roots`. |
| `CM_CURSOR_RULES_ENABLED` | Enable rule file injection (default **true**; set `0`/`false` to disable). |
| `CM_CURSOR_RULES_DIR` | Directory of `*.mdc`. |
| `CM_CURSOR_RULES_INCLUDE_AGENTS_MD` | Append `AGENTS.md`. |
| `CM_CURSOR_RULES_MAX_CHARS` | Max injected chars. |

**Path safety (matches implementation)**: `workspace_allowed_roots` and per-request revalidation catch `..` escapes and symlinks that already point outside roots **at check time**. On **Unix**, **`read_file`** (`resolve_for_read_open`) and Web workspace list/read/write/delete go through **`src/workspace/fs.rs`**: on Linux, **`openat2` + `RESOLVE_IN_ROOT`** opens paths relative to an already-open workspace-root fd, narrowing the race between policy checks and `open`; symlinks inside the tree may still be followed, but resolution cannot escape the root. **Residual risk**: checks still depend on `canonicalize` at check time; non-Linux paths and code that does not use `workspace_fs` may still be TOCTOU-prone; **`create_dir_all`** + opens are not fully atomic. This is **not** a kernel sandbox; use **Web auth** on open networks. See **`src/workspace/path.rs`**.

### Planning

| Variable | Description |
| --- | --- |
| `CM_FINAL_PLAN_REQUIREMENT` | `never` / `workflow_reflection` / `always`. |
| `CM_PLAN_REWRITE_MAX_ATTEMPTS` | Max plan rewrite rounds. |
| `CM_INTENT_L2_ENABLED` | Enable default L2 no-tools semantic intent classification (extra `chat`; falls back to deprecated rules on failure; **default on**). TOML: `intent_l2_enabled`. |
| `CM_INTENT_L2_MIN_CONFIDENCE` | Observation threshold for L2 `confidence` (0.0–1.0, default 0.7); below still uses L2, only flags in log/timeline. TOML: `intent_l2_min_confidence`. |
| `CM_INTENT_L2_MAX_TOKENS` | L2 classification `max_tokens` (32–1024, default 384). TOML: `intent_l2_max_tokens`. |
| `CM_INTENT_L0_ROUTING_BOOST_ENABLED` | Conservative L0-feature boost for ambiguous sentences (default on). TOML: `intent_l0_routing_boost_enabled`. |
| `CM_INTENT_EXECUTE_LOW_THRESHOLD` | Deprecated-rule fallback "confirm then execute" low threshold (0.0–1.0, default 0.2). TOML: `intent_execute_low_threshold`. |
| `CM_INTENT_EXECUTE_HIGH_THRESHOLD` | Deprecated-rule fallback "execute directly" high threshold, ≥ low (default 0.45). TOML: `intent_execute_high_threshold`. |
| `CM_INTENT_NON_HIER_EXECUTE_LOW_THRESHOLD` | Non-hierarchical override for "confirm then execute" low; falls back to `CM_INTENT_EXECUTE_LOW_THRESHOLD`. TOML: `intent_non_hier_execute_low_threshold`. |
| `CM_INTENT_NON_HIER_EXECUTE_HIGH_THRESHOLD` | Non-hierarchical override for "execute directly" high, ≥ non-hier low; falls back to `CM_INTENT_EXECUTE_HIGH_THRESHOLD`. TOML: `intent_non_hier_execute_high_threshold`. |
| `CM_INTENT_MODE_BIAS_ENABLED` | Whether hierarchical `runner` lightly biases execution mode by `primary_intent` (default on). TOML: `intent_mode_bias_enabled`. |

### Intent gates vs `plan_rewrite` (quick reference)

| Mechanism | When it applies | Relation to **`plan_rewrite_max_attempts`** |
| --- | --- | --- |
| **`intent_execute_low_threshold` / `intent_execute_high_threshold`** | Turn-start **`intent_at_turn_start`**: confidence bands for “confirm then execute” vs “execute directly”, etc. | **None** — does not consume rewrite budget |
| **`intent_non_hier_execute_*`** | Same stack, but overrides the two thresholds when **`planner_executor_mode != hierarchical`**; falls back to **`intent_execute_*`** if unset | **None** |
| **`intent_at_turn_start` (gate)** | First in non-hierarchical dispatch; may end the turn early (clarify / confirm / QA, …) or set hints | **None** |
| **`plan_rewrite_max_attempts`** | After an **`agent_reply_plan` v1** (or equivalent final-plan artifact) exists: invalid plan, semantic side-check feedback, … | Independent of intent thresholds; exhaustion → SSE **`plan_rewrite_exhausted`** (**`docs/en/SSE_PROTOCOL.md`**) |

**Clarify / confirm and tool narrowing**: **`ClarifyThenExecute`** / **`ConfirmThenExecute`** set **`step_executor_constraint = ReviewReadonly`** before the main loop (same idea as **`qa.readonly`** narrowing).

### Queue, parallelism, cache

| Variable | Description |
| --- | --- |
| `CM_HEALTH_LLM_MODELS_PROBE` | When `1`/`true`, **`GET /health`** runs a **GET …/models** check (list endpoint only, no completion cost). Default off. |
| `CM_HEALTH_LLM_MODELS_PROBE_CACHE_SECS` | Cache probe results in-process (**5–86400**, default **120**) to limit upstream traffic from frequent health polls. |
| `CM_CHAT_QUEUE_MAX_CONCURRENT` | Max concurrent chat jobs. |
| `CM_CHAT_QUEUE_MAX_PENDING` | Max queued chat jobs. |
| `CM_PARALLEL_READONLY_TOOLS_MAX` | Max parallel readonly tools per round. |
| `CM_READ_FILE_TURN_CACHE_MAX_ENTRIES` | Per-turn `read_file` cache; `0` off; cleared on writes / workspace change. |
| `CM_TEST_RESULT_CACHE_ENABLED` | In-process test output LRU. |
| `CM_TEST_RESULT_CACHE_MAX_ENTRIES` | LRU size. Reuses truncated output for `cargo_test`, `rust_test_one`, `npm_run` (`script=test`), `run_command` `cargo`+`test` without `--nocapture` / `--test-threads`; first line **`[CrabMate test output cache hit]`**; not across restarts. |

### Session workspace changelist

| Variable | Description |
| --- | --- |
| `CM_SESSION_WORKSPACE_CHANGELIST_ENABLED` | Inject `crabmate_workspace_changelist` user message. |
| `CM_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS` | Max injected chars. Accumulates writes + unified diff per `long_term_memory_scope_id` (Web: `conversation_id`; CLI default `__default__`); not in session SQLite (stripped on save). **`workflow_execute` node tools** excluded. |

### Allowlist, MCP, conversation store

| Variable | Description |
| --- | --- |
| `CM_ALLOWED_COMMANDS` | Comma-separated allowlist for **`run_command`** and the first **`terminal_session` `exec`**. Embedded defaults also include **`bash`** / **`sh`** (for **`bash -c` / `sh -c`** compound one-liners), **`docker`**, **`podman`**, **`mvn`**, **`gradle`**, …; full list **`config/tools.toml`**. |
| `CM_MCP_ENABLED` | Enable MCP. Requires **`cargo build --features mcp`**; without that feature, `mcp list` and in-process MCP tool proxy are unavailable. |
| `CM_MCP_COMMAND` | MCP stdio launch command. |
| `CM_MCP_TOOL_TIMEOUT_SECS` | MCP tool timeout; one stdio session per fingerprint; **`crabmate mcp list`** needs no `API_KEY`; **`mcp list --probe`** spawns subprocess. |
| `CM_CODEBASE_SEMANTIC_SEARCH_ENABLED` | Register **`codebase_semantic_search`** (`false` removes from tool list). |
| `CM_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH` | Relative semantic index SQLite path; default **`.crabmate/codebase_semantic.sqlite`**. |
| `CM_CODEBASE_SEMANTIC_MAX_FILE_BYTES` | Max bytes per indexed file. |
| `CM_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS` | Max chars per chunk. |
| `CM_CODEBASE_SEMANTIC_TOP_K` | Default Top-K. |
| `CM_CODEBASE_SEMANTIC_REBUILD_MAX_FILES` | Max files **re-embedded** per **`rebuild_index`** (large-repo guard; unchanged files are skipped in incremental mode). |
| `CM_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL` | Workspace-wide **`rebuild_index`** defaults to **incremental** (**`mtime` + `size` + SHA256**); **`false`** clears chunk + file-catalog rows then full re-embed. Subtree **`path`** still replaces that prefix only. |
| `CM_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS` | Max vector chunks scanned per **`query`** (default **50000**; **0** = unlimited). |
| `CM_CODEBASE_SEMANTIC_HYBRID_ALPHA` | Default **`retrieve_mode: hybrid`** vector weight **α** (0–1): **α×cosine + (1-α)×fts_norm** (SQLite **FTS5** BM25 normalized). |
| `CM_CODEBASE_SEMANTIC_FTS_TOP_N` | Max FTS rows for hybrid / **`fts_only`** (BM25); **1–10000**, default **400**. |
| `CM_CODEBASE_SEMANTIC_HYBRID_SEMANTIC_POOL` | Hybrid: vector candidate pool size (≥ **`top_k`**); **1–10000**, default **256**. |
| `CM_CONVERSATION_STORE_SQLITE_PATH` | Conversation SQLite path. Embedded default **`.crabmate/conversations.db`** (relative to the active workspace); set to empty to disable server-side transcript persistence (in-memory mode). |

### First-turn injection

| Variable | Description |
| --- | --- |
| `CM_MEMORY_FILE_ENABLED` | Workspace memo file injection. |
| `CM_MEMORY_FILE` | Memo path. |
| `CM_MEMORY_FILE_MAX_CHARS` | Memo max chars. |
| `CM_LIVING_DOCS_INJECT_ENABLED` | Prepend a short summary from **`.crabmate/living_docs/`** (`SUMMARY.md`, `map.md`, …) to the first-turn merged `user` block; embedded default **on** (nothing is injected when no Markdown files qualify). |
| `CM_LIVING_DOCS_RELATIVE_DIR` | Living-docs directory relative to workspace root (default `.crabmate/living_docs`). |
| `CM_LIVING_DOCS_INJECT_MAX_CHARS` | Total char budget for living-docs injection; `0` disables. |
| `CM_LIVING_DOCS_FILE_MAX_EACH_CHARS` | Per-file read budget under that directory. |
| `CM_PROJECT_PROFILE_INJECT_ENABLED` | Project profile injection. |
| `CM_PROJECT_PROFILE_INJECT_MAX_CHARS` | Profile max chars. |
| `CM_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED` | Dependency brief (merged with profile/memo). |
| `CM_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS` | From `cargo metadata` edges + Mermaid + **`package.json` name excerpts** under the **workspace root or a `frontend/` subdirectory** (common npm layout). **Only paths that actually contain `package.json`** contribute; this does not collide with this repo’s Leptos **`frontend/`** tree (usually no `package.json`); `0` disables segment. |

### Tool explain card

| Variable | Description |
| --- | --- |
| `CM_TOOL_CALL_EXPLAIN_ENABLED` | Require `crabmate_explain_why` on mutating tools. |
| `CM_TOOL_CALL_EXPLAIN_MIN_CHARS` | Min explain length. |
| `CM_TOOL_CALL_EXPLAIN_MAX_CHARS` | Max explain length. |

### Long-term memory

| Variable | Description |
| --- | --- |
| `CM_LONG_TERM_MEMORY_ENABLED` | Enable long-term memory. |
| `CM_LONG_TERM_MEMORY_SCOPE_MODE` | Scope mode. |
| `CM_LONG_TERM_MEMORY_VECTOR_BACKEND` | TOML default `fastembed` or `disabled`. Requires **`cargo build --features fastembed`** for runtime embeddings; without that feature, **`finalize`** downgrades `fastembed` to `disabled` (SQLite long-term memory still works). |
| `CM_LONG_TERM_MEMORY_STORE_SQLITE_PATH` | SQLite for vectors/metadata. |
| `CM_LONG_TERM_MEMORY_TOP_K` | Retrieval Top-K. |
| `CM_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK` | Max chars per chunk. |
| `CM_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX` | Min chars to index. |
| `CM_LONG_TERM_MEMORY_ASYNC_INDEX` | Async indexing. |
| `CM_LONG_TERM_MEMORY_AUTO_INDEX_TURNS` | After each turn, auto-index last user/assistant pair; `false` keeps only explicit **`long_term_remember`** writes. |
| `CM_LONG_TERM_MEMORY_DEFAULT_TTL_SECS` | Default TTL seconds for **auto**-indexed rows; `0` = no expiry (still capped by **`max_entries`**). Explicit **`long_term_remember`** can set `ttl_secs` per call. |
| `CM_LONG_TERM_MEMORY_MAX_ENTRIES` | Max entries. |
| `CM_LONG_TERM_MEMORY_INJECT_MAX_CHARS` | Max chars injected into model context. |

Injected lines are prefixed with **`[memory #id]`** where **`id`** is the SQLite **`crabmate_long_term_memory`** primary key—align with **`long_term_memory_list`** or debugging.

Expired rows are purged on read/write. Built-in tools **`long_term_remember`**, **`long_term_forget`**, **`long_term_memory_list`** are registered when **`long_term_memory_enabled`** (do not store secrets).

Embedded defaults set **`conversation_store_sqlite_path`** to **`.crabmate/conversations.db`**, so **Web `serve`** (and **`tui`** with the same path) persist transcripts and can resume a **`conversation_id`** after restart; clear the path to revert to in-memory sessions. Session and memory **may** share one SQLite; long-term memory still defaults to **`run_command_working_dir/.crabmate/long_term_memory.db`**. If long-term memory is enabled but the DB cannot open: one **stderr** warning, process continues without injection.

### Web search & `http_fetch`

| Variable | Description |
| --- | --- |
| `CM_WEB_SEARCH_PROVIDER` | Provider id. |
| `CM_WEB_SEARCH_API_KEY` | Search API key. |
| `CM_WEB_SEARCH_TIMEOUT_SECS` | Timeout seconds. |
| `CM_WEB_SEARCH_MAX_RESULTS` | Max results. |
| `CM_HTTP_FETCH_ALLOWED_PREFIXES` | Allowed URL prefixes. |
| `CM_HTTP_FETCH_TIMEOUT_SECS` | Fetch timeout. |
| `CM_HTTP_FETCH_MAX_RESPONSE_BYTES` | Max response bytes. |

**Outer `tokio::time::timeout` around `spawn_blocking`**: besides **`http_fetch_timeout_secs`** (client read timeout), the async path wraps blocking work. Defaults align with **`command_timeout_secs`** and **`http_fetch_timeout_secs`**. Override with TOML **`[tool_registry]`** keys **`http_fetch_wall_timeout_secs`** / **`http_request_wall_timeout_secs`** (see commented examples at the end of **`config/tools.toml`**).

### `tool_registry` policy (`tools.toml` / main config)

Optional table **`[tool_registry]`** in **`config/tools.toml`** or your **`config.toml`** (merged like other fragments) maps into **`AgentConfig`** and is updated on hot reload. **No `CM_*` aliases**—use TOML.

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
| `CM_MAX_MESSAGE_HISTORY` | Max messages kept. |
| `CM_TOOL_MESSAGE_MAX_CHARS` | Compress `role: tool` before model if longer. |
| `CM_TOOL_RESULT_ENVELOPE_V1` | `crabmate_tool` envelope v1. |
| `CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS` | When truthy, SSE **`tool_call`** includes redacted, length-capped **`arguments`** in addition to **`arguments_preview`** (default off; reduces accidental exposure in the browser). |
| `CM_TOOL_STATS_ENABLED` | When truthy, enable in-process tool-outcome stats and append a short hint to the **new** conversation’s first `system` (see below). |
| `CM_TOOL_STATS_WINDOW_EVENTS` | Sliding-window event cap (16–65536); mirrors TOML `agent_tool_stats_window_events`. |
| `CM_TOOL_STATS_MIN_SAMPLES` | Min total calls per tool in the window before it appears in the hint (1–10000). |
| `CM_TOOL_STATS_MAX_CHARS` | Max Unicode scalars for the appendix (64–32768; truncated if longer). |
| `CM_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO` | Hint if success rate is below this (0.0–1.0) and `min_samples` is met; failures always qualify. |
| `CM_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS` | Materialize DeepSeek DSML tool calls. |
| `CM_DSML_STREAM_STRIP_ENABLED` | Strip DSML markers from streaming SSE/CLI/TUI deltas before display; defaults to the materialize flag when unset. TOML: `dsml_stream_strip_enabled`. |
| `CM_THINKING_AVOID_ECHO_SYSTEM_PROMPT` | Append the thinking-discipline appendix to the first `system` message; defaults to on. |
| `CM_THINKING_AVOID_ECHO_APPENDIX` | Inline appendix body (non-empty clears the file path; if **`…_FILE`** is set afterward, **file wins**). |
| `CM_THINKING_AVOID_ECHO_APPENDIX_FILE` | Path to appendix Markdown (same resolution as **`system_prompt_file`**). |
| `CM_CONTEXT_CHAR_BUDGET` | Character budget trim. |
| `CM_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM` | Min messages after system post-summary. |
| `CM_CONTEXT_SUMMARY_TRIGGER_CHARS` | Trigger summary when over char threshold. |
| `CM_CONTEXT_SUMMARY_TAIL_MESSAGES` | Tail messages kept after summary. |
| `CM_CONTEXT_SUMMARY_MAX_TOKENS` | Summary request max_tokens. |
| `CM_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS` | Summary transcript max chars. |

**`[agent]` TOML keys (tool stats)**: `agent_tool_stats_enabled`, `agent_tool_stats_window_events`, `agent_tool_stats_min_samples`, `agent_tool_stats_max_chars`, `agent_tool_stats_warn_below_success_ratio`. Stats are **per-process**, **global** (not bucketed by `conversation_id`); **no** tool args or full outputs stored. Web attaches the stats appendix only for **new** chats (no stored seed). CLI `chat` / `repl` and `workspace_session::initial_workspace_messages` attach on fresh first-`system` paths; sessions loaded from disk keep base system alignment **without** the stats appendix.

**Workspace + user-query dynamic selection**: With **`skills_enabled`** and **`skills_top_k`** (**`CM_SKILLS_TOP_K`**), Web and CLI (`chat`/REPL) can merge Top-K snippets from **`.crabmate/skills`** into the first **`system`** each turn. First-turn project profile, living docs, and dependency brief use **`project_profile_*` / `living_docs_*` / `project_dependency_brief_*`** and land in a **dedicated `user`**, separate from **`system`**. This can improve relevance but trades off retrieval error, latency, and **`CM_CONTEXT_*`** / **`CM_TOOL_MESSAGE_MAX_CHARS`** budgets; treat the workspace as a **trust boundary** (see **`.cursor/rules/security-sensitive-surface.mdc`**).

### CLI

| Variable | Description |
| --- | --- |
| `CM_TUI_LOAD_SESSION_ON_START` | Load session from disk on start. |
| `CM_TUI_SESSION_MAX_MESSAGES` | Max messages in session file. |
| `CM_TUI_CONVERSATION_ID` | **`tui` only**: when **`conversation_store_sqlite_path`** is set, bind this **`conversation_id`** at startup (same charset rules as Web); if unset, an id **`tui-…`** is generated. |
| `CM_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED` | If `true`, background `initial_workspace_messages` (profile, deps); default `false`. TOML: `[agent] repl_initial_workspace_messages_enabled`. |
| `CM_CLI_WAIT_SPINNER` | If truthy, show indicatif spinner on stderr before first stream chunk in CLI/`chat` (needs TTY stderr, not **`NO_COLOR`**). |

### Docker tool sandbox

| Variable | Description |
| --- | --- |
| `CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE` | `none` \| `docker`. |
| `CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE` | Required image in `docker` mode. |
| `CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK` | Empty = no network; `bridge` for outbound tools. |
| `CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS` | Per-container wait cap. |
| `CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER` | Docker `Config.user`; `current`/`host` semantics in § SyncDefault Docker below. |

You may also use **`DOCKER_HOST`** (non-`CM_`) like the `docker` CLI / bollard.

```bash
export CM_MODEL=deepseek-reasoner
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

Default **`thinking_avoid_echo_system_prompt = true`** (**`[agent]`**, embedded default in **`config/default_config.toml`**, same section as **`system_prompt_file`**). Appendix text defaults from **`thinking_avoid_echo_appendix_file`** (shipped **`config/prompts/thinking_avoid_echo_appendix.md`** — edit on disk without rebuilding); optional **`thinking_avoid_echo_appendix`** inline string. **Precedence**: non-empty **`thinking_avoid_echo_appendix_file`** is read from disk **before** inline; if neither is set, a compile-time embedded default is used. **`tool_stats::augment_system_prompt`** appends the resolved body to the **first `system`** of **new** Web/CLI chats. **Soft** hint only. Disable with **`thinking_avoid_echo_system_prompt = false`** or **`CM_THINKING_AVOID_ECHO_SYSTEM_PROMPT=0`**.

## Zhipu GLM (OpenAI-compatible)

**`api_base`**: **`https://open.bigmodel.cn/api/paas/v4`** (do not append `/chat/completions`). **`model`**: e.g. **`glm-5`**. **`API_KEY`** as Bearer.

Minimal vendor-style request: **`model`**, **`messages`**, **`stream: true`** without **`thinking`**. CrabMate with **`llm_bigmodel_thinking = false`** omits **`thinking`**; Web/CLI streaming uses **`stream: true`**.

Optional deep thinking: **`llm_bigmodel_thinking = true`** (**`CM_LLM_BIGMODEL_THINKING=1`**) → **`thinking: { "type": "enabled" }`** per [GLM-5 docs](https://docs.bigmodel.cn/cn/guide/models/text/glm-5).

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

## Volcano Engine Ark (OpenAI-compatible, incl. Coding Plan)

If **`api_base`** uses a Volcano host (**`*.volces.com`**, e.g. **`https://ark.cn-beijing.volces.com/api/coding/v3`**), CrabMate **does not apply Moonshot-hosted Kimi request shaping**, so it **does not emit Moonshot-only fields** like **`thinking`** that Ark rejects with HTTP **400 InvalidParameter**, and it **omits MiniMax-only `reasoning_split`** even if **`CM_LLM_REASONING_SPLIT`** / **`llm_reasoning_split`** were enabled elsewhere. Set **`model`** exactly as the console shows (e.g. **`Kimi-K2.6`**). Use your Ark **`API_KEY`** with **`llm_http_auth_mode = bearer`**.

## DeepSeek (OpenAI-compatible)

**`api_base`** containing **`deepseek`** (e.g. **`https://api.deepseek.com/v1`**) selects the DeepSeek vendor adapter (after Kimi/MiniMax/Zhipu routing). Per [DeepSeek thinking mode](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode), CrabMate may send **`thinking: {"type":"enabled"|"disabled"}`** and, when explicitly enabling, **`reasoning_effort: "high"`** on **`chat/completions`** requests.

- **`llm_bigmodel_thinking = true`** (**`CM_LLM_BIGMODEL_THINKING=1`**, or Web **`client_llm.llm_thinking_mode: on`**) → **`thinking` enabled** + **`reasoning_effort: high`**.
- **`llm_kimi_thinking_disabled = true`** (Web **`llm_thinking_mode: off`** sets this) → **`thinking` disabled**; **`reasoning_effort`** omitted. If both flags apply, **disabled wins** (same precedence as Kimi).
- Neither flag → omit both fields; gateway defaults apply (docs: thinking **enabled** by default).

Hierarchical **Manager** JSON paths still strip **`thinking`**, **`reasoning_split`**, and **`reasoning_effort`**.

## Sample `config.toml`

```toml
[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-reasoner"
# system_prompt = "…"
# system_prompt_file = "my_prompt.txt"
# cursor_rules_enabled = false   # default true; if `.cursor/rules` or `*.mdc` are absent, behavior matches off
# cursor_rules_dir = ".cursor/rules"
```

## Final answer plan (`final_plan_requirement`)

When the model ends a turn **without** `tool_calls`, whether an embeddable **`agent_reply_plan`** JSON is required (details: **[DEVELOPMENT.md](DEVELOPMENT.md)**).

- **`workflow_reflection`** (default): require plan only after workflow reflection path.
- **`never`**: no enforcement.
- **`always`** (experimental): every final answer—**higher cost**.

With `workflow_validate_only` results, **`spec.layer_count`** constrains step count. Optional **`workflow_node_id`** must be a subset of **`nodes[].id`** from the latest **`workflow_execute`** result.

**Strict node coverage (`final_plan_require_strict_workflow_node_coverage`, default `false`, `CM_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE`)**: when `true`, if **any** step sets `workflow_node_id`, the plan must reference **every** `nodes[].id` from the latest workflow tool result at least once. If no step sets `workflow_node_id`, this rule does not apply.

**Optional semantic side-check LLM (default off)**: **`final_plan_semantic_check_enabled`** (`CM_FINAL_PLAN_SEMANTIC_CHECK_ENABLED`, default `false`) with **`final_plan_requirement = workflow_reflection`**: after static checks pass, if a tool digest can be built from history, one extra no-tools `chat/completions` asks whether the plan contradicts recent tool output. The side model should reply with JSON: `{"consistent":true}` or `{"consistent":false,"violation_codes":["…"],"rationale":"…"}` (legacy one-line **`CONSISTENT` / `INCONSISTENT`** still accepted). On inconsistent, the rewrite user message includes a fenced JSON block **`crabmate_plan_semantic_feedback` v1** with **`violation_codes`** (and optional **`rationale`**) before the usual plan-rewrite instructions; this counts against **`plan_rewrite_max_attempts`**. **`final_plan_semantic_check_max_non_readonly_tools`** (`CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS`, default `0`, range 0–32) caps extra non-readonly tool lines in the digest; at `0`, high-risk builtin names (e.g. `run_command`, `workflow_execute`) and readonly tools may still appear. **`final_plan_semantic_check_max_tokens`** (`CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS`, default `256`, clamp 32–1024) sets side-call `max_tokens`. Parse/API failures **fail open** (treat as consistent).

## Plan rewrite (`plan_rewrite_max_attempts`)

Max “please rewrite” user injections when the plan is invalid; when exhausted, stream may emit **`code: plan_rewrite_exhausted`** (optional sibling **`reason_code`**, see **`docs/en/SSE_PROTOCOL.md`**).

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

Env: `CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE`, `CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`, etc.

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

- **Default**: **`system_prompt_file = "config/prompts/base_system_prompt.md"`** (domain-neutral L0; read at runtime; edit without rebuild). When **`coding_workbench_enabled = true`** (default), **`finalize`** also appends **`coding_workbench_increment_file`** (default **`config/prompts/coding_workbench_increment.md`**). Disable or replace L0b via **`[agent] coding_workbench_enabled`** / **`coding_workbench_increment_file`**, or **`CM_CODING_WORKBENCH_ENABLED`** / **`CM_CODING_WORKBENCH_INCREMENT_FILE`** (falls back to embedded body on read failure). Per-role **`prepend_coding_workbench`** (default true; still gated by the global flag) skips L0b when false — companion / philosopher / literary ship with false.
- **Relative path resolution**: process **cwd** → each overlay **config file directory** (later wins, e.g. `.agent_demo.toml` before `config.toml`) → **`run_command_working_dir`**. **Absolute** paths tried as-is.
- **Overrides**: Inline **`system_prompt`** without **`system_prompt_file`** in a layer **drops** inherited file for that layer. Env: **`CM_SYSTEM_PROMPT`** clears merged file; **`CM_SYSTEM_PROMPT_FILE`** wins if both set.
- **finalize**: Read file if **`system_prompt_file`** set; else non-empty inline; else error.
- **Shipped default body**: **`config/prompts/base_system_prompt.md`** (L0) plus **`coding_workbench_increment.md`** (L0b for dev workflows). Instruction precedence, tool discipline, and communication norms are split across these files and optional **`.cursor/rules`**. Fully custom: replace those files or set `system_prompt_file`.
- **Embedded defaults** (`config/default_config.toml`): **`thinking_avoid_echo_system_prompt = true`** with **`thinking_avoid_echo_appendix_file = "config/prompts/thinking_avoid_echo_appendix.md"`** (override via inline **`thinking_avoid_echo_appendix`** or **`CM_THINKING_AVOID_ECHO_APPENDIX*`**); see § *Reduce system-prompt echo in thinking chains* above.

## Multi-role (agent_roles)

Besides the global `system_prompt`, you can define **named ids** with their own first-turn `system` text (each merged with **`cursor_rules_*`** and a lightweight skills index at finalize; full skills bodies are injected per-turn by L5).

- **Sources** (later overlays win for the same id):  
  1. **`[[agent_roles]]`** rows in the main config: **`id`**, plus **`system_prompt`** and/or **`system_prompt_file`**. Empty inline **`system_prompt`** means **inherit** the global merged system. Optional **`prepend_coding_workbench`** (default true): when false, that role skips L0b (still gated by global **`coding_workbench_enabled`**).  
  2. **`config/agent_roles.toml`** when not using **`--config`**; with **`--config path/to/foo.toml`**, read **`path/to/agent_roles.toml`** next to it. Shape: **`[agent_roles]`**, optional **`default_role`**, **`[agent_roles.roles.<id>]`** (see `config/agent_roles.toml`).
- **Default role**: **`[agent] default_agent_role`**, or **`agent_roles.toml` `[agent_roles] default_role`**, or **`CM_DEFAULT_CM_ROLE`**. Must reference a defined id; if unset, omitting `agent_role` uses the global **`system_prompt`**.
- **Optional `allowed_tools` (multi-role workbench)**: On **`[[agent_roles]]`** rows or **`[agent_roles.roles.<id>]`**, you may set a string array **`allowed_tools`**. When non-empty, that role may call **only** those built-in tool names; include the literal **`mcp`** to allow all **`mcp__*`** MCP proxy tools. Omit or use an empty list for **no restriction** (legacy behavior). The effective named id for tool policy follows **`agent_role` request → persisted `active_agent_role` → `default_agent_role_id`**, aligned with the first `system` message role.
- **Web**: optional JSON **`agent_role`** on **`POST /chat`** / **`POST /chat/stream`**. **New session** (no stored history for **`conversation_id`**): same as before, seeds first-turn `system`. **Existing session**: if **`agent_role`** differs from persisted **`active_agent_role`**, the server **refreshes only the first `system`** and updates the stored role, **keeping** the rest of the transcript; omitting **`agent_role`** keeps the last persisted role. With **`allowed_tools`**, each turn filters tools sent to the model and rejects disallowed execution.
- **CLI**: global **`--agent-role <id>`** for **`repl`** / **`chat`**. Mutually exclusive with **`chat --system-prompt-file`**. For **`chat`** without **`--messages-json-file`**, applies to the first-turn system (including **`--message-file`** first line); **`allowed_tools`** apply the same way as Web.
- **REPL**: **`/agent list`** prints **`default`** then configured ids (same as **`GET /status`** **`agent_role_ids`**). **`/agent set <id>`** / **`/agent set default`**: validate id, update the in-memory role, and **replace only the first `system`** (**do not** clear the transcript) for mid-session persona switches; **`default`** clears the explicit named role.
- **Hot reload**: role table reloads with **`POST /config/reload`** / **`/config reload`**.
- **`GET /status`**: **`agent_role_ids`**, **`default_agent_role_id`**.

## Cursor-like rules

When **`cursor_rules_enabled`** (**default `true`**), append sorted **`cursor_rules_dir`/*.mdc** (optional **`AGENTS.md`**) to system prompt, capped by **`cursor_rules_max_chars`**. If the directory is missing or no rule files load, nothing is appended (same effect as disabled).

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
