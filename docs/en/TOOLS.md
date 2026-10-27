**Languages / 语言:** [中文](../工具说明.md) · English (this page)

# CrabMate built-in tools reference

This document describes built-in tools, common function-calling JSON examples, and release-check troubleshooting. Workspace browsing, streaming, and session export remain in the root [`README.md`](../../README.md) “Features”; day-to-day use also starts there.

**Optional “explain card”** (`tool_call_explain_enabled`): When enabled, every **non-read-only** built-in tool (same notion as `tool_registry::is_readonly_tool(&AgentConfig, name)`, including `run_command`, `cargo_test` / `cargo_run` / `pytest_run`, file writers, `http_request`, **`gh_api`**, mutating git, `workflow_execute`, …; overridable via **`[tool_registry] write_effect_tools`**) must include top-level string **`crabmate_explain_why`** describing intent in natural language; the server validates length and **strips** it before execution to avoid conflicting with `additionalProperties: false`. Read-only tools and MCP proxies do not require it; MCP still strips the key before forwarding. Complements command/HTTP **approval** (approval authorizes, explain card improves traceability).

**Long outputs in model context** (`tool_result_envelope_v1`, default on): History `role: tool` uses the **`crabmate_tool`** JSON envelope (`summary`, `output`, …). Optional **`structured_payload`** bundles stable **`schema`** values: **`run_command_exit_v1`** (`run_command`), **`subprocess_exit_v1`** (`cargo_*`; matches `{title} (exit=N):` first line), **`http_tool_response_v1`** (`http_fetch` / `http_request` successful bodies with `method:` / `请求 URL:` / `状态:` lines; omitted on prefix errors / allowlist misses). The envelope also includes **`tool_call_id`**, **`execution_mode`** (`serial` / `parallel_readonly_batch`), **`parallel_batch_id`** (shared within a parallel read-only batch); failures include **`error_code`**, optional **`failure_category`** (stable bucket aligned with **`tool_result::ToolFailureCategory`**, derived from `error_code`; see **`docs/en/SSE_PROTOCOL.md`**), and **`retryable`** (heuristic with `error_code`, not a guarantee). **Parallel read-only batches** use the same wall-clock as serial **`dispatch_tool`** (`tool_registry::parallel_tool_wall_timeout_secs`; e.g. **`http_fetch`** uses **`max(http_fetch_timeout_secs, command_timeout_secs)`**, **`get_weather` / `web_search`** use their own timeouts, others often **`command_timeout_secs`**); on timeout the text mentions timeout and **`tool_result.error_code`** is **`timeout`**. Before each model request, if over **`tool_message_max_chars`**, the server head/tail-samples **`output`** and sets **`output_truncated`**, **`output_original_chars`**, **`output_kept_head_chars`**, **`output_kept_tail_chars`** so one grep/build log cannot fill the window; full text may still appear via SSE/export depending on UI. SSE **`tool_result`** carries the same correlation fields; **`structured_preview`** may merge first-line **`crabmate_tool_output`** with **`structured_payload`**. Pre-run SSE **`tool_call`** also includes **`arguments_preview`** (single-line truncation aligned with `execute_tools` logs); when **`sse_tool_call_include_arguments`** / **`CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS`** is on, the server adds heuristically redacted **`arguments`** (longer cap, still not a full-fidelity guarantee). See **`docs/en/DEVELOPMENT.md`**, **`docs/en/SSE_PROTOCOL.md`**, **`docs/en/CONFIGURATION.md`**. **Transparent tool-failure retry** (**`[tool_registry] tool_retry_*`**, off by default): when enabled, read-only approval-free tools (`http_fetch` requires a URL matching `http_fetch_allowed_prefixes`, `read_dir` requires a non-external path; `codebase_semantic_search` and other tools whose params can write side effects are hard-excluded) transparently retry transient errors (default `timeout` / `http_timeout` / `rate_limited` / `http_network_error`) at the dispatch layer with backoff `min(backoff*2^(n-1), 5000)`; intermediate failures are **not** written to model context, only the final result is; user cancel stops retrying. See **`docs/en/CONFIGURATION.md`**.

**`run_command` + Web diff preview**: When the **stdout** section (after CrabMate’s `run_command` framing) looks like a **git unified diff** (first non-empty line starts with **`diff --git`** or **`diff --cc`**), or the invocation is **`git … status`** (after skipping common globals like **`-C` / `--git-dir`**) and stdout looks like typical **`git status`** output (e.g. **`On branch` / `##`**, **`nothing to commit`**, **`Changes not staged`**, short **`XY `** porcelain lines, …), SSE **`tool_result.structured_preview`** also attaches the same **`workspace_write_diff`**-shaped header as file-write tools, and the Web tool card reuses the “Change preview (unified diff)” block (subject to the same preview size cap).

**Session workspace changelist** (default on, `session_workspace_changelist_*`): Successful writes from **`create_file` / `modify_file` / `copy_file` / `move_file` / `delete_files` / `append_file` / `search_replace` / `apply_patch` / `structured` (`action=patch`)** accumulate **relative paths** and a unified diff vs first touch this session; before **each** model request a **`user`** is injected (**`name=crabmate_workspace_changelist`**). Web strips this before saving sessions (like long-term memory injection). Tools inside **`workflow_execute`** DAG use a separate `ToolContext` and **do not** hit this table.

**Final answer `agent_reply_plan` v1** (validated on workflow reflection / final-plan gate paths): `steps[].id` must be unique and match stable character rules (enforced by validators alongside **`workflow_execute`**; see JSON examples below). Optional **`workflow_node_id`** aligns with **`nodes[].id`** from the latest **`workflow_execute`** tool result (subset check). Optional **`steps[].executor_kind`** (`review_readonly` / `patch_write` / `test_runner`) may still appear in plan JSON for tooling/docs; the **staged step runner / `step_verifier` path has been removed**—runtime orchestration is **session_mode → (Act utterance heuristics) → ReAct**. See **`docs/en/CONFIGURATION.md`**.

**Copy-paste plan JSON (placeholders, one step per rolling plan round is typical)**:

```json
{
  "type": "agent_reply_plan",
  "version": 1,
  "steps": [
    {
      "id": "read-only-survey",
      "description": "Read-only scan of YOUR_ANCHOR_FILE.md",
      "executor_kind": "review_readonly",
      "max_step_retries": 1
    }
  ]
}
```

```json
{
  "type": "agent_reply_plan",
  "version": 1,
  "steps": [
    {
      "id": "run-checks",
      "description": "Run a placeholder check and assert stdout/exit code",
      "executor_kind": "test_runner",
      "max_step_retries": 2,
      "acceptance": {
        "expect_exit_code": 0,
        "expect_stdout_contains": "YOUR_EXPECTED_SUBSTRING"
      }
    }
  ]
}
```

**Optional Docker sandbox** (`sync_default_tool_sandbox_mode = docker`): **SyncDefault** and **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** run in a **one-shot Docker container** (bollard → Engine API) after host approval/allowlist; **`workflow_execute`** and **MCP** stay on the host. Requires reachable **Docker daemon** and non-empty **`sync_default_tool_sandbox_docker_image`** with the CLIs you need (`git`, `rg`, `cargo`, …) and **same CPU arch** as the host `crabmate` binary (mounted read-only; repo ships no fixed image). Default **no network** (`docker_network` empty); for weather/search/HTTP egress set **`sync_default_tool_sandbox_docker_network`** (e.g. `bridge`). Steps, sample Dockerfile, env, and safety notes: [`docs/en/CONFIGURATION.md`](CONFIGURATION.md) § SyncDefault Docker sandbox.

## Built-in tools (model-callable)

**Choosing `run_command` vs `terminal_session`:** Prefer **`run_command`** for non-interactive, one-shot commands (builds, queries, scripts that exit when done). Use **`terminal_session`** only when the program needs a **TTY** (full-screen interaction, raw terminal control, or multiple rounds of input/output in the **same** pseudo-terminal). **`terminal_session`** is **Linux PTY only** and is unavailable when **`sync_default_tool_sandbox_mode = docker`**. Do not use **`terminal_session`** for ordinary batch commands.

**`run_command` timeout and cancel:** Wall clock defaults to **`command_timeout_secs`** (`CM_COMMAND_TIMEOUT_SECS`). On expiry, user cancel, or SSE close, CrabMate sends SIGTERM then SIGKILL to the child **process group** (Unix) and returns truncated partial stdout/stderr with envelope **`error_code`** `timeout` / `cancelled`. On the host chat path, live output is also sent as existing SSE **`tool_output_chunk`** (`tool_call_id`, monotonic **`seq`**, `stream`=`stdout`/`stderr`; **not** model context; still capped by **`command_max_output_len`**). Chunks are queued in **pipe-read order** (not PTY-accurate interleaving); a client that ignores `stream` concatenates both. If the SSE channel is full, the same delta is retried without skipping `seq`. Workflow nodes currently reap this way only for **`run_command`** (no SSE chunks); other tools may still leave `spawn_blocking` children. Docker-sandbox processes are out of P0/P1 scope. See **`docs/design/long_running_tool_execution_todo.md`**.

**Compound shell (globs / `$VAR` / `~` / `&&` / `|`, …):** `command` + `args` are joined into a **full script**. When **`allowed_commands` includes `bash` or `sh`** (both in the embedded default), glob/`$VAR`/`~` run via **`bash -c` / `sh -c`**. Standalone argv operators (`&&` / `||` / `|` / `;`) also wrap, but **Web always re-approves** that script so they cannot silently bypass the single-command allowlist. `?` / `[` are not glob unless the token looks like a path glob (`file?.c`, not `rg -e foo?`). `cd dir && git status` peels the `cd` prefix before this policy. Unknown commands and expansion-without-shell-on-allowlist go through Web approval of **that script**. Plain argv with no expansion (e.g. `git status`) still uses `execve`. Drop **`bash`/`sh`** from the allowlist to forbid arbitrary scripts. First **`terminal_session` `exec`** follows the same rule.

- **Many built-in tools; the model picks as needed**:
  - `get_weather`: Weather by city/region ([Open-Meteo](https://open-meteo.com/), no key).
  - `web_search`: **Web search**. Default **`web_search_provider = worbrow`** ([worbrow](https://crates.io/crates/worbrow) **≥0.2.0** drives local **Firefox / Chrome/Edge** headless; engine chain **bing→duckduckgo**; **no API key**; alias `browser`; results may include quality flags such as ads/dictionary). Optional **`brave`** ([Brave Search API](https://brave.com/search/api/)) or **`tavily`** ([Tavily](https://tavily.com/)) with `web_search_api_key` / `CM_WEB_SEARCH_API_KEY`. Prefer `search_in_files` for exact string/regex; use `codebase_semantic_search` for **hybrid FTS + vector** retrieval (default `retrieve_mode: hybrid`; needs `rebuild_index`; see below and **`docs/en/CONFIGURATION.md`**).
  - `http_fetch`: **GET** (default) or **HEAD**. GET returns status, Content-Type, **redirect chain**, body (timeouts/size caps); **HEAD** skips body. Body decoding: **`Content-Type` charset**, HTML **`<meta charset>`** / **http-equiv=Content-Type**, `<?xml encoding=…?>`, else **`chardetng`** sniffing; `application/json` prefers UTF-8; binary types (`image/*`, etc.) still use lossy UTF-8 preview. Optional **`text_format: html_text`**: turn **HTML documents** into plain text with **`scraper` (html5ever)** (skip `script`/`style`/`noscript`, prefer `main` / `article` / `[role=main]`, else `body`); non-HTML / JSON / binary responses keep the decoded source and note why in the decode line. Requests are curl-aligned: automatically send **`Accept: */*`** and **`Accept-Encoding`** (gzip/brotli/deflate; response bodies are decompressed); **`User-Agent`** defaults to **`crabmate/<version>`**, overridable via **`http_fetch_user_agent`** / **`CM_HTTP_FETCH_USER_AGENT`** (e.g. a browser or curl UA for anti-bot sites). Environment proxies follow **`ALL_PROXY` / `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`** (HTTP(S) proxies work; **reqwest 0.13 does not support SOCKS** — with a `socks5://` proxy, the tool fails and hints at unsetting or using the same port as `http://`). **URL gate**: `http_fetch_allowed_prefixes` entry **`"*"`** allows any **http/https** immediately (embedded default; still rejects `file:` and other schemes). Otherwise match **same origin + path prefix boundary**. Unmatched URLs: **Web only** (`/chat/stream` + `approval_session_id`) can approve via SSE **deny / once / always** (GET/HEAD share normalized whitelist key; ops CLI has no in-process terminal approval). **`"*"` lets the model fetch loopback / RFC1918 / cloud metadata URLs** (SSRF); on multi-tenant or exposed `serve`, use concrete prefixes or an explicit empty list (`[]` / empty env) to override the embed (empty = all URLs need approval). `http_request` uses the same list. **Failure error codes**: `http_fetch` / `http_request` failure bodies carry an explicit first-line marker `错误[code]：` (server-classified precisely) — connect/DNS failures are **`http_network_error`**, request timeouts **`http_timeout`** (envelope `retryable=true`), response-body read failures **`http_body_read_error`**; `get_weather` network/API failures are **`weather_api_error`**, `web_search` **`web_search_api_error`**, and both timeouts are **`timeout`**.
  - `http_request`: **POST / PUT / PATCH / DELETE** (optional `json_body`). Same `http_fetch_allowed_prefixes` rules (`"*"` = any http/https); unmatched URLs use the same **Web SSE** approval path (**permanent key** `http_request:<METHOD>:<URL>` vs `http_fetch:`). **`workflow_execute` nodes** still require a prefix match (no approval on sync path). Returns status, Content-Type, redirects, body (same decoding and optional **`text_format: html_text`** as `http_fetch`; dry-run first; never put real secrets in bodies).
  - **GitHub CLI wrappers** (local **`gh`**, **`allowed_commands` must include `gh`**): **PR** — **`gh_pr`** (dispatches on **`action`**: `checks` (optional **`structured: true`** failure summary) / `create` (**writes**; default **`auto_body`** from template + git log when `body` omitted) / `merge` / `review` / `comment` / `edit` (**writes**) / `body_draft` (read-only)); **Issue** — **`gh_issue_create`**; **Actions** — **`gh_run`** (`action=failure_summary` for failed jobs + log tails / `action=rerun`, writes); **Release** — **`gh_release_create`** (optional **`auto_notes`**); **`gh_api`**. Pretty JSON appended on exit 0 when stdout is JSON. `gh_pr` / `gh_run` are treated as non-read-only as a whole (including the read-only `checks` / `body_draft` / `failure_summary` actions). The read-only list/view/search wrappers (`gh pr list`, `gh pr view`, `gh pr diff`, `gh issue list`, `gh issue view`, `gh run list`, `gh run view`, `gh release list`, `gh release view`, `gh search`) were removed in favor of raw `run_command`.
  - `run_command`: When `allowed_commands` is `*` (`serve --yes` or `CM_ALLOWED_COMMANDS=*`), the command name is not checked and everything is allowed. When argv needs glob / `$VAR` / `~` and **`bash`/`sh`** are allowlisted, join `command`+`args` and run **`bash -c`**. Web re-approves standalone `&&` / `|` even if bash is already allowlisted. Approval shows the full script. Plain argv still `execve`s. You may still pass **`bash`/`sh`** with **`args` `["-c","whole script"]`**. Whitelisted read/query Linux commands (`ls`, `pwd`, `whoami`, `date`, `cat`, `file`, `head`, `tail`, `wc`, `cmake`, `ctest`, `mkdir`, `ninja`, `gcc`, `g++`, `clang`, `clang++`, `c++filt`, `autoreconf`, `autoconf`, `automake`, `aclocal`, `make`, GNU Binutils read-only tools `objdump`, `nm`, `readelf`, `strings`, `size`, default also `ar`, …) with timeout and output truncation. **GitHub CLI**: allowlist includes **`gh`** (install locally and authenticate, e.g. `gh auth login`); same arg rules as `git`—no `..` and no `/`-prefixed args—use `owner/repo` or relative paths, not absolute filesystem paths in arguments. Missing CLI → **`dep_gh`** degraded on **`GET /health`**; **`crabmate doctor`** reports `gh` availability in the toolchain section. **CMake/ctest** are allowlisted; args must not contain `..` or start with `/`; prefer relative build dirs (avoid absolute `-D`). Missing tools may show `dep_cmake` / `dep_ctest` degraded on `/health`. **mkdir**: creates dirs (complements **`create_dir`**). **c++filt**: demangle C++ symbols. **Binutils**: missing → corresponding `dep_*` degraded. **Autotools**: trusted workspaces only. **Test output cache** (`test_result_cache_enabled`): when **`command` is `cargo` and `args` start with `test`** and **omit** `--nocapture` / `--test-threads`, shares in-process LRU with `cargo_test`; hits prefix output with **`[CrabMate test output cache hit]`**.
  - `terminal_session`: **Linux PTY only** (`forkpty` in the workspace). First **`action":"exec"`** without **`session_id`** requires **`command`** (optional **`args`**), same allowlist/approval rules as **`run_command`**; for compound commands prefer **`bash`/`sh` + `-c`** on that **`exec`** (do not split `&&` across **`args`**); if an interactive shell is already up, **`input`** can carry a full line. Streaming output via SSE **`tool_output_chunk`**, finalized in **`tool_result`**. **`list` / `close` / `resize` / `send_signal` / `interrupt`**: **`interrupt`** writes **ETX** (`\x03`, i.e. Ctrl-C) to the session's foreground process group; **`send_signal`** targets the **foreground process group** (`tcgetpgrp`), falling back to the session leader when unavailable. **Cancellation of the tool call / outer execution timeout** recycles the session (avoiding long-term use of the 8-session cap); the per-round read wall is `command_timeout_secs`, which only ends the current round and **keeps the session** open for a later `exec`. Unavailable when **`sync_default_tool_sandbox_mode = docker`**; up to **8** concurrent sessions; output caps follow **`command_exec`** (`command_max_output_len`).
  - `run_executable`: Run a **relative-path** executable under the workspace (e.g. `./main`, build artifacts). Use this for workspace binaries—not `run_command`.
  - **Package info (now via `run_command`)**: `dpkg-query` / `rpm` are on the `run_command` allowlist (`config/tools.toml`), so call them directly (e.g. `dpkg-query -W -f='${Status} ${Version}\n' <pkg>`, `rpm -q <pkg>`). The former thin wrapper (`package_query`) was removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; cost is per §2.1 (loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache, and no longer provides the apt/rpm abstraction or structured JSON output).
  - `delete_files` (needs `confirm`; `paths` list, up to 32; rejects the whole batch if any path is invalid) / `delete_dir` (needs `confirm`, optional `recursive`).
  - `append_file`: Append to file (`create_if_missing` optional, `dry_run` previews only). By default `ensure_leading_newline=true`, so a non-empty file without a trailing newline gets one before new content is appended.
  - `create_dir`: Create directory (default `parents=true`, `mkdir -p` style).
  - `search_replace`: Single-file replace (literal or regex; default `dry_run`; write needs `confirm`). **Preferred tool for localized content edits** — text-anchored with no line numbers, so it is immune to line-number drift and less likely than line-based `modify_file` to hit the wrong line.
  - `create_file` / `modify_file`; before **`create_file`**, **`modify_file`**, or **`append_file`** runs, the server normalizes common **`path`** aliases (e.g. **`file_path`**, **`filename`**, **`file`**, **`name`**, **`output_path`**) and **`content`** aliases (**`body`**, **`text`**, **`code`**, …); key matching is case-insensitive; for **`create_file`**, omitted **`content`** is treated as an empty string. If streamed `arguments` are invalid JSON, local execution **keeps the raw string** (tool reports JSON parse error) and only the vendor round-trip sanitizes to `{}`, so you do not get a misleading missing-`path` Schema error. `read_file` supports ranges, line caps, **`anchor_line`/`context_lines`** (symmetric window around a search hit; mutually exclusive with `start_line`/`end_line`), **`encoding`** (`utf-8` strict, `utf-8-sig`, `gb18030`/`gbk`/`big5`, `utf-16le`/`be`, `auto`, …; malformed → error). For broad code analysis, prefer shrinking the candidate set with `search_in_files` / `codebase_semantic_search` (index required) before segmented reads. `modify_file` can replace line ranges or insert after a line. **Per-turn** `read_file` cache (mtime+size; cleared on writes / workspace change), key includes **encoding**, cap **`read_file_turn_cache_max_entries`**. Web `GET /workspace/file` caps **1 MiB**, same decoding, optional **`encoding`**. Tool-generated **png/jpg/jpeg/webp/gif** (e.g. from `run_command` / `python_snippet_run`) **must** be embedded in the **final assistant reply** as **`![alt](relative/path.png)`** (Web loads **`GET /workspace/file/raw`**). Do **not** tell users they cannot inline images, or to copy files onto static-site directories / `/uploads`. Successful tool results that mention those paths get a `[crabmate] Web 聊天内嵌…` footer. No svg; 8 MiB cap. Paths in outputs are **relative to workspace** (POSIX), not host absolutes.
  - `copy_file` / `move_file`: Copy/move **files** inside workspace (path safety like `create_file`); default no overwrite unless `overwrite: true`; cross-device `move_file` copies then deletes source.
  - **Write precheck (`write_precheck`)**: `create_file` / `modify_file` (`mode=full/overwrite`) / `append_file` / `search_replace` run a syntax smoke check on the **content about to be written**, keyed by extension: `.py` (Python `compile`, covers `IndentationError` and `return outside function`), `.json` (`serde_json`), `.toml` (`toml`). On failure the write is **refused** and the tool returns `校验失败（CODE）：path:line: message` (`CODE` = `PY_SYNTAX_ERROR` / `JSON_SYNTAX_ERROR` / `TOML_SYNTAX_ERROR`; envelope `error_code` = **`write_precheck_failed`**, `failure_category=invalid_input`) so the model can fix by line number and retry. `skip_precheck=true` bypasses explicitly (progressive edits / intermediate states); a missing checker (e.g. no `python3`) or timeout skips silently without blocking the write. `modify_file` `replace_lines` / `insert_after_line` (streaming local edits) are not prechecked.
  - `read_dir` / `glob_files` / `list_tree`: List dir; glob recurse; tree with `max_depth` / `max_entries` caps, stay inside workspace.
  - `codebase_semantic_search`: **Hybrid** code search (SQLite **FTS5** + **fastembed** vectors, separate from long-term memory; **`crabmate_codebase_chunks_fts`** external-content table + triggers; **`crabmate_codebase_files`** fingerprints). **`rebuild_index: true`** → **`.crabmate/codebase_semantic.sqlite`** (schema **v4**+). **Incremental** workspace-wide rebuild by default; subtree **`path`** replaces that prefix only. **Rust** symbol hint lines before embed. **`query`** default **`retrieve_mode: hybrid`** (BM25 + cosine via **`hybrid_alpha`**); **`semantic_only`** / **`fts_only`**. **`fts_top_n`**, **`hybrid_semantic_pool`**, **`query_max_chunks`** tune cost. **`codebase_semantic_rebuild_max_files`** caps re-embeds. Not read-only. Disable: `codebase_semantic_search_enabled = false`. Invalidation: chunk deletes sync FTS via triggers; broad tools may full-clear.
  - `markdown_check_links`: Scan Markdown (default `README.md`, `docs/`) for relative links and `#fragment` anchors; `output_format=text|json|sarif`. External `http(s)://` default offline; optional `allowed_external_prefixes` enables HEAD probes (deduped cache).
  - **Spell / AST search**: `typos`, `codespell`, and `ast-grep` are on the `run_command` allowlist—call them via `run_command` (spell check needs the CLI installed; `ast-grep run --rewrite` needs an explicit `--update-all` to write, with no `confirm` gate).
  - `structured`: Structured-data tool dispatching on **`action`** (`validate` / `query` / `diff` / `patch`) — validate, path lookup, key-level structured diff, and point `set`/`remove` on **JSON / YAML / TOML** (`action=patch` defaults to dry-run; `dry_run=false` writes; the sub-verb is **`mutation`**=`set`/`remove`). CSV/TSV stay validate/query/diff only — no structured write-back. **The whole tool counts as a write** (the accepted cost of the merge): `action=validate` / `query` / `diff` no longer get the parallel read-only batch, read-only retry, or `Ask`·`Plan` read-only gating, and `ReviewReadonly` steps cannot call it.
  - `table_text`: Preview/validate/filter/aggregate delimiter tables (CSV/TSV/; / |, streaming, 4 MiB/file); complements `structured` (`action=validate|query|diff`) “load whole table as JSON”.
  - `text_diff`: Line unified diff for two UTF-8 strings or two workspace files (not Git); complements `structured` (`action=diff`).
  - `repo_overview_sweep`: Read-only **rollup**: optional **project profile** (same as sidebar / `GET /workspace/profile`: `Cargo.toml` / workspace, `package.json`, top dirs, tokei, optional `cargo metadata --no-deps`; `include_project_profile`, `project_profile_max_chars`, default on, 6000 cap; **not** gated by the `project_profile_inject_enabled` master switch, and **not injected on the first turn**); doc previews (default `README.md`, `AGENTS.md`, `docs/开发文档.md`, …); `list_tree` on `src` (`source_roots`); globs for manifests/CI; ends with an **outline for conclusions**. **No LLM inside**—the model writes analysis from this output. Options: `doc_paths`, `doc_preview_max_lines`, `list_tree_*`, `build_globs`, ….
  - `crate_contract_map`: Read-only **contract map**: fixed anchors for **HTTP/SSE/config** (e.g. `src/lib.rs`, `docs/SSE协议.md`, `src/cm_sse_protocol/control_classify.rs`, `fixtures/sse_control_golden.jsonl`, `config/default_config.toml`, `docs/配置说明.md`, `docs/命令行与路由.md`) with **short file heads** and **keyword line excerpts** per section (no subprocess, no network, no `.env`). Official UI parsers live in the Client repo `frontend/src/api/` (this workspace has no `frontend/`). Options: `head_lines_per_file`, `keyword_hits_per_file`, `extra_paths` (relative, head-only), `max_extra_paths`. Complements `repo_overview_sweep` (repo skeleton vs protocol/routing entrypoints).
  - `docs_health_sweep`: Doc previews + spell check (typos / codespell) + `markdown_check_links`. Missing CLIs → **skipped** steps. **External links**: only if **`md_allowed_external_prefixes`** non-empty does `markdown_check_links` use the **built-in HTTP client** for HEAD; **not** `http_fetch`/`http_request`, **not** `http_fetch_allowed_prefixes`, **no** Web SSE approval. Empty prefix → count only, no network. Options: `fail_fast`, `summary_only`, `spell_paths`, per-step toggles, ….
  - `diagnostic_summary`: Redacted diagnostics—Rust toolchain (`rustc`/`cargo`/`rustup`/`bc`), workspace `target/`, common `Cargo.toml` / `frontend` paths, whether key env vars **are set** (**never values**; no length for secrets). Optional `extra_env_vars` (safe uppercase names).
  - `self_config_info`: Read-only query of the **runtime-merged self config** (env overrides, hot reload, Web key injection all applied), so the model can answer “self-config” questions (model, `api_base`, sampling, timeouts, workspace roots, …): LLM gateway (`model`/`api_base`/`auth_mode`/`planner_model`/`executor_model`), sampling (`max_tokens`/`temperature`/`context_tokens`/`seed`), vendor flags, HTTP retry, command exec (timeout/output cap/allowlist count), weather, web search (provider / whether `api_key` **is set**), controlled HTTP fetch, plan policy, roles (default agent role/session mode/role count), workspace roots (allowed roots/pool/current session), Web API (whether Bearer **is set**/required/CORS), context pipeline, turn budget, long-term memory, conversation store path, MCP, tool registry policy, sandbox. Optional `sections` array limits output (e.g. `llm`, `sampling`, `web_search`, …); **secret fields only report whether they are set, never their values**.
  - `long_term_remember` / `long_term_forget` / `long_term_memory_list`: Explicit long-term memory tools (requires **`long_term_memory_enabled`** and a mounted DB for the session). `remember` supports optional `tags` and `ttl_secs` (`0` = no expiry); `forget` deletes by `memory_id` or exact `memory_text`; `list` shows recent rows. **Do not store secrets.**
  - ~~`error_output_playbook`~~ / ~~`playbook_run_commands`~~: **removed** on 2026-10-27 (whole tools) — their suggestions came from a fixed "ecosystem × category" lookup table rather than reasoning about the specific error; the model can classify the error itself and call `run_command` directly. The former "missing CLI → install hint" path is now up to the model reading `run_command`'s failure output.
  - **Python** (workspace root; CLIs must exist): `pytest_run` (`python3 -m pytest`), `python_snippet_run`. **Format**: `format_files` was retired on 2026-10-20 — call the formatters through `run_command` (`rustfmt` / `cargo fmt`, `ruff format`, `clang-format -i`, `gofmt -w`, `shfmt -w`, `xmllint --format`, `sqlfluff format`, `prettier --write`); all are in `allowed_commands`. **Tag filtering**: `build_tools_with_options` / `ToolsBuildOptions` + `dev_tag` (`python`, `cpp`, …; see `src/cm_tools/tools/dev_tag.rs`). **Custom LLM backend**: `RunAgentTurnParams.transport.llm_backend` optional `ChatCompletionsBackend`. New-tool checklist: `AGENTS.md` → Repository engineering rules → Tool registry.
  - **JVM / Container / Node frontend (now via `run_command`)**: `mvn`/`gradle` (or `gradlew`), `docker`/`podman`, and `npm`/`npx`/`tsc` are all on the `run_command` allowlist (`config/tools.toml`), so call them directly via `run_command` (e.g. `mvn -q test`, `gradle classes`, `docker compose ps`, `podman images`, `npm run build`, `npx tsc --noEmit`). The former thin wrappers (`maven_*`/`gradle_*`/`docker_build`/`docker_compose_ps`/`podman_images`/`npm_install`/`npm_run`/`npx_run`/`tsc_check`/`frontend_lint`/`frontend_build`/`frontend_test`) were removed per design doc §8.5.4 **track A**; the JVM implementation (`jvm_tools`) was **deleted entirely**, and the container/frontend commands are now called through `run_command` directly (`docker` / `podman` / `npm` / `npx` are allowlisted).
  - **Go (now via `run_command`)**: `go`/`gofmt`/`golangci-lint` are all on the `run_command` allowlist (`config/tools.toml`), so call them directly (e.g. `go build ./...`, `go test ./...`, `go vet ./...`, `gofmt -l .`, `golangci-lint run`). The former thin wrappers (`go_build`/`go_test`/`go_vet`/`go_fmt_check`/`golangci_lint`) were removed as a family per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; cost is per §2.1 (loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache; the `go_` prefix was already excluded from the parallel read-only batch). Note: the former tools skipped when `go.mod` was absent — that guard is gone with `run_command`.
  - **Python (now via `run_command`)**: `ruff`/`mypy`/`uv` are all on the `run_command` allowlist (`config/tools.toml`), so call them directly (e.g. `ruff check .`, `mypy src`, `uv sync`, `uv run pytest -q`). The former thin wrappers (`ruff_check`/`mypy_check`/`uv_sync`/`uv_run`) were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; cost is per §2.1 (loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache; the `ruff_`/`mypy_`/`uv_` prefixes were already excluded from the parallel read-only batch). The `ruff_check` / `mypy_check` function bodies were shared by the `run_lints` aggregator, which was removed on 2026-10-28, so both bodies went with it. `pytest_run` is **kept** because of its background-job wiring.
  - **pre-commit (now via `run_command`)**: `pre-commit` is on the `run_command` allowlist (`config/tools.toml`), so call it directly (e.g. `pre-commit run --all-files`, `pre-commit run <hook> --files <path>`). The former thin wrapper (`pre_commit_run`) was removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; cost is per §2.1 (loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache; the `pre_commit` prefix was already excluded from the parallel read-only batch). Note: the former tool skipped when `.pre-commit-config.yaml` / `.yml` was absent — that guard is gone with `run_command`.
  - **Process/port (now via `run_command`)**: `ps` / `ss` / `lsof` are on the `run_command` allowlist (`config/tools.toml`), so call them directly (e.g. `ps aux`, `ss -ltnp`, `lsof -i :<port>`). The former thin wrappers (`port_check` / `process_list`) were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; cost is per §2.1 (loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache, and no longer post-processes process/port output).
  - **Background jobs** (retrieve results of long-running `run_command` started with `async: true` in the same session; reads the in-process registry, no allowlist approval): `background_job` (**dispatches on `action`**; all three actions are read-only and `status` / `output` require `tool_job_id`; fields irrelevant to the current `action` are ignored, but `limit` is range-checked (1–100) for **every** action, so passing it with `action=status` fails validation instead of being dropped — `status` queries status/output by `tool_job_id` (returns current state while running, exit code plus stdout/stderr when finished), `list` lists jobs for the workspace newest first with optional `limit` (default 20 / max 100), `output` does incremental output pull, `tail -f` semantics; pass `tool_job_id` and optional `cursor` (the previous `next_cursor`), returns stdout/stderr deltas and prints the cursor plus an `eof` marker at the end of the text; omit `cursor` to start from the earliest available; at most 500 items per call, keep pulling with the cursor while not `eof`), `background_job_cancel` (cancel a job: `queued` moves straight to `cancelled`, `running` sets the cancel flag, already-`cancelled` is idempotent success, other terminal states cannot be cancelled). **Error codes** (both are typed runners since 2026-10-28): `invalid_args` for a missing/blank `tool_job_id`, unknown or missing `action`, or a malformed argument shape; a context without the in-process registry also returns `invalid_args` (call it from the main conversation turn). The old `参数错误：`tool_job_id` 不能为空…` body neither started with `错误` nor contained `失败` / `超时`, so `parse_legacy_output` judged it a **success** — a model that forgot the id was told the query worked.
  - **Schedule** (writes `.crabmate/reminders.json` / `.crabmate/events.json`; nine tools before the merge): `reminder` (**dispatches on `action`** — `list` read-only, `add` / `update` / `delete` / `complete` write) and `event` (`list` read-only, `add` / `update` / `delete` write). `update` / `delete` / `complete` require `id`; `add` requires a non-empty `title` (`event` also `start_at`). Both count as write side effects, so they stay out of the parallel read-only batch, the read-only TTL cache and the same-turn dedup cache. **Error codes** (both are Typed runners since 2026-10-28, so failure paths are explicit): `invalid_args` (bad arg JSON / missing required field / unknown or missing `action`), `reminder_not_found` / `event_not_found` (`id` does not exist — categorised `invalid_input`, so the model can `action=list` and self-correct), `schedule_io_failed` (creating `.crabmate` / reading / writing failed — `external`), `schedule_parse_failed` (the existing JSON file is corrupt), `schedule_serialize_failed` (`internal`). Before this, three branches — missing `title` on `add`, missing `id`, and an unknown `id` — produced bodies with no 错误 / 失败 keyword and were therefore judged **successful** by `parse_legacy_output`; they are now explicit failures.
  - **Metrics/analysis** (read-only): `code_stats` (tokei/cloc/fallback), `dependency_graph` (Cargo/Go/npm, Mermaid/DOT/tree), `coverage_report` (LCOV, Tarpaulin JSON, Cobertura XML).

## How permission levels affect tools

`permission_level` (`chat` / `read` / `workspace` / `approved` / `unrestricted`, default **`approved`**) decides both *which tools are advertised* and *whether a registered tool may run*. Design: [`../design/permission_levels.md`](../design/permission_levels.md); keys: `docs/配置说明.md` § 权限等级.

| Level | Tool visibility | MCP / dynamic tools | Path scope | Commands | Outbound network | Out-of-bounds |
|---|---|---|---|---|---|---|
| `chat` | **no tool definitions** | denied | unreachable | all denied | all denied | denied |
| `read` | read-only allowlist | denied | session workspace root | all denied | prefix allowlist | denied |
| `workspace` | full | **denied** | session workspace root | allowlist | prefix allowlist | **denied, no prompt** |
| `approved` | full | allowed | session workspace root | allowlist | prefix allowlist | human approval |
| `unrestricted` | full | allowed | any | allowlist ignored | any | no prompt, audit only |

- `run_command` / `terminal_session` / `python_snippet_run` / `run_executable` follow the **command axis**: hard-denied at `chat` / `read` (even if the model replays an old `tool_call`), denied at `workspace` when outside the allowlist, approval at `approved`, no allowlist check at `unrestricted`.
- File tools (`read_file` / `write` / `read_dir`, …) share one path carrier: `unrestricted` accepts absolute paths and `..`; `chat`–`approved` stay inside the session workspace root (out-of-bounds at `approved` goes through `WorkspaceExternalPath` approval).
- `http_fetch` / `http_request` follow the **network axis** (`http_fetch_allowed_prefixes`); on a prefix miss `workspace` denies, `approved` prompts, `unrestricted` allows.
- `mcp__*` and `plugins/*.json` dynamic tools are **denied at `chat` / `read` / `workspace`**, allowed at `approved` / `unrestricted`.
- **Irreversible-operation confirms are not waived**: `delete_files` / `delete_dir` still require `confirm` under `unrestricted` (see `permission_confirm_tools`), and `run_command` still gates dangerous argv (`git reset --hard`, `git push --force`).
- `AllowAlways` keys are level-scoped (e.g. `l3|git`): a hit requires an **exact** level match, so lowering the session level invalidates earlier approvals (same for `http_fetch` / `http_request` permanent keys).
- Timeline entries (`TOOL_CALL_START.permissionLevel`) and approval dialogs (`command_approval.permissionLevel`) carry the level for **display only**; no allow/deny decision reads them.
- **Known gap**: `PathScope::AllowedRoots` is not wired yet, so `workspace_allowed_roots` does not widen `workspace` / `approved` paths; the sandbox does not narrow the network axis yet and `clamped_by` never reports `sandbox`.

---

## GitHub CLI (`gh_*`) examples

- `gh_pr` (**dispatches on `action`**; `checks` / `body_draft` read-only, `create` / `merge` / `review` / `comment` / `edit` **write to GitHub**):
  ```json
  {"action":"checks","structured":true}
  ```
  ```json
  {"action":"checks","repo":"octocat/Hello-World","number":42,"structured":true}
  ```
  ```json
  {"action":"body_draft","base":"main","max_commits":20}
  ```
  ```json
  {"action":"create","title":"fix: handle null / fix null deref"}
  ```
  ```json
  {"action":"create","repo":"octocat/Hello-World","title":"feat: API","body":"## Summary\n- …","base":"main","head":"my-feature","draft":true}
  ```
  ```json
  {"action":"merge","number":42,"delete_branch":true}
  ```
  (Omit **`merge_method`** to default to **`rebase`**; or set `merge` / `squash` / `rebase` explicitly.)
  ```json
  {"action":"merge","number":42,"merge_method":"squash","delete_branch":true}
  ```
  ```json
  {"action":"review","number":42,"event":"approve"}
  ```
  ```json
  {"action":"comment","number":42,"body":"LGTM."}
  ```
  ```json
  {"action":"edit","number":42,"title":"feat: new title","add_label":["bug"],"draft":true}
  ```
- `gh_issue_create` (**writes**):
  ```json
  {"title":"bug: login fails","body":"Steps…","labels":["bug"]}
  ```
- `gh_run` (`action=failure_summary` read-only; `action=rerun` **writes**, optional `failed: true` reruns failed jobs only):
  ```json
  {"action":"failure_summary","run_id":"12345678901","tail_lines":80,"max_failed_jobs":3}
  ```
  ```json
  {"action":"rerun","run_id":"12345678901","failed":true}
  ```
- `gh_release_create` (**writes**; **`auto_notes: true`** drafts notes since last tag):
  ```json
  {"tag":"v1.2.0","title":"v1.2.0","auto_notes":true}
  ```
- `gh_api` (read-only GET; mutating methods need `crabmate_explain_why` when explain-card is on):
  ```json
  {"path":"repos/octocat/Hello-World/pulls","method":"GET"}
  ```
  `path` is a **relative endpoint path only** (no leading `/`, restricted charset) and **must not contain a query string `?`**; pass query parameters via the **`query`** object, e.g. `{"path":"repos/octocat/Hello-World/issues","query":{"state":"open","per_page":5}}`. The tool converts `query` to `-f key=value` and always passes `--method` explicitly, so a request won't be flipped to POST just because `gh` sees `-f`; use `extra_args` only for the few flags `query` doesn't cover.

## Rust dev tool examples

Structured function-calling JSON examples:

- `cargo_run`:
  ```json
  {"bin":"crabmate","args":["--help"]}
  ```
- Run a single test with `cargo_test` + `test_filter` (the former `rust_test_one` was removed — it was only sugar for this, and it was also misclassified as read-only):
  ```json
  {"test_filter":"tools::tests::test_build_tools_names","nocapture":true}
  ```
- **Local CI / pre-release combined checks (now via `run_command`)**: the three aggregate tools `ci_pipeline_local` / `release_ready_check` / `quality_workspace` were removed as a family (see [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §8.5); use `run_command` chaining with `bash -c` (`cargo` / `npm` / `python3` / `git` are all allowlisted):
  ```json
  {"command":"bash","args":["-c","cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test"]}
  ```
  ```json
  {"command":"bash","args":["-c","cargo audit && cargo deny check advisories licenses bans sources && git status --porcelain"]}
  ```
  The former aggregators' `fail_fast` / `summary_only` / per-section summary and the "skip when no `Cargo.toml`" guard are **gone** — chain with `&&` and read the full output.
- `rust_analyzer` (one tool with an `action` selector — the eight former `rust_analyzer_*` tools were merged into it; **`rust-analyzer` must be in PATH**; **line/character are 0-based**, as in LSP):
  ```json
  {"action":"goto_definition","path":"src/cm_tools/mod.rs","line":1195,"character":4,"wait_after_open_ms":500}
  ```
  ```json
  {"action":"find_references","path":"src/cm_tools/mod.rs","line":1195,"character":4,"include_declaration":true}
  ```
  ```json
  {"action":"document_symbol","path":"src/lib.rs","max_symbols":200}
  ```
  ```json
  {"action":"workspace_symbol","path":"src/lib.rs","query":"run_agent_turn","max_results":32}
  ```
  **Actions and required fields**: `goto_definition` / `find_references` / `hover` / `goto_implementation` / `goto_type_definition` / `document_highlight` need `path` + `line` (LSP `textDocument/definition`, `/references`, `/hover`, `/implementation`, `/typeDefinition`, `/documentHighlight`); `document_symbol` needs `path` (`max_symbols` default 500, max 5000); `workspace_symbol` needs `path` (primes `didOpen` on a Rust file) + `query` (`max_results` default 64, max 500). A missing or unknown `action` returns an error listing every accepted value.
  **Failures and error codes** (typed runner, 2026-10-28): success bodies are byte-identical to before; every failure now carries an `error_code` instead of relying on body keywords — `invalid_args` (missing / unknown `action`, malformed argument JSON, missing `path` / `line` / `query`, absolute path or `..`, path escaping the workspace or not a file, file >512 KiB), `rust_analyzer_file_not_found` (target file does not exist: `canonicalize` failed), `rust_analyzer_read_failed` (could not read the source file), `rust_analyzer_spawn_failed` (`rust-analyzer` would not start: not installed / not permitted / wrong `server_path`), `rust_analyzer_lsp_failed` (LSP read/write or protocol error, **and** an `error` object inside the rust-analyzer response, e.g. `method not found`), `rust_analyzer_workspace_resolve_failed` (the workspace root itself is unresolvable — category `Workspace`), `timeout` (LSP read timeout, 25 s), `rust_analyzer_internal` (missing child-process pipes: internal defect). **Two kinds of body used to be classified as success**: `路径无法解析: No such file or directory` and the `LSP error` bodies emitted by the response formatters — the model read "the file does not exist" or "LSP rejected the request" as "no information at this position", which is why this tool needed a typed runner rather than a wording fix.
- `rust_backtrace_analyze`: **removed** on 2026-10-27 (whole tool) — it was a pure-computation analyzer; the model can read the panic text directly, or locate the suspect frame via `run_command` (`c++filt` is on the default allowlist) and `code_nav`.
- `frontend_lint` / `frontend_build` / `frontend_test` (frontend lint / build / test): removed per §8.5.4 **track A**; use `run_command` instead (e.g. `npm run lint` / `npm run build` / `npm run test`).
- `workflow_execute` (DAG: parallelism, approval, SLA, compensation):
  - **Built-in template**: set **`workflow.workflow_template`** (currently **`rust_ci_light`**) to expand a serial **`run_command` (`cargo fmt --check`) → `cargo_check` (`all_targets`) → `cargo_clippy` (`all_targets`) → `cargo_test`** DAG without hand-writing **`nodes`**. Mutually exclusive with a fully custom **`nodes`** unless you override after expansion: other **`workflow`** keys merge on top; a provided **`nodes`** array replaces the template’s nodes.
  - Node **`max_retries`** (0–5, default 0): auto backoff retry for **`timeout`**, **`workflow_tool_join_error`**, **`workflow_semaphore_closed`**, …; **not** for business failures (tests, non-zero exit) to avoid duplicate side effects.
  - **Static check**: each **`tool_name`** must be a built-in tool; **`tool_args`** must include schema **`required`** keys (recursive into nested objects/arrays; full validation still in runners).
  - **Result JSON** (`workflow_execute_result` / `workflow_validate_result`): **`workflow_run_id`** (matches logs), **`trace`** (`dag_start`, `node_attempt_*`, `node_retry_backoff`, `dag_end`, …), **`completion_order`**, **`nodes[].attempt`** final count.
  ```json
  {"workflow":{
    "max_parallelism":2,
    "fail_fast":true,
    "compensate_on_failure":true,
    "nodes":[
      {"id":"clean","tool_name":"run_command","tool_args":{"command":"cargo","args":["clean","--dry-run"]},"deps":[],"compensate_with":[]},
      {"id":"clippy","tool_name":"cargo_clippy","tool_args":{"all_targets":true},"deps":["clean"],"compensate_with":["clean"]},
      {"id":"test","tool_name":"cargo_test","tool_args":{},"deps":["clippy"],"compensate_with":[]},
      {"id":"deny","tool_name":"run_command","tool_args":{"command":"cargo","args":["deny","check","advisories","licenses","bans","sources","--all-features"]},"deps":["test"],"requires_approval":true,"compensate_with":["clean"]}
    ]
  }}
  ```
 - `workflow_execute` (Git: inject `git log` hash into `git show`):
   ```json
   {"workflow":{
     "max_parallelism":2,
     "fail_fast":true,
     "compensate_on_failure":false,
     "nodes":[
       {"id":"log","tool_name":"git","tool_args":{"action":"log","max_count":1,"oneline":true},"deps":[],"compensate_with":[]},
       {"id":"show","tool_name":"git","tool_args":{"action":"show","rev":"{{log.stdout_first_token}}"},"deps":["log"],"compensate_with":[]}
     ]
   }}
   ```

 - `workflow_execute` (Git: diff base + patch approval):
   ```json
   {"workflow":{
     "max_parallelism":1,
     "fail_fast":true,
     "compensate_on_failure":false,
     "nodes":[
       {"id":"diff","tool_name":"git","tool_args":{"action":"diff","base":"main","context_lines":3},"deps":[],"compensate_with":[]},
       {"id":"patch_check","tool_name":"run_command","tool_args":{"command":"git","args":["apply","--check","patches/fix.diff"]},"deps":["diff"],"compensate_with":[]},
       {"id":"patch_apply","tool_name":"run_command","tool_args":{"command":"git","args":["apply","patches/fix.diff"]},"deps":["patch_check"],"requires_approval":true,"compensate_with":[]}
     ]
   }}
   ```

   `patch_path` must point to an existing workspace patch (e.g. `patches/fix.diff`) you or a prior step created; write operations (e.g. `git apply`) are downgraded to `run_command` per §8.5.4 **track B**.

  Downstream string fields may use placeholders (recursive into JSON objects/arrays):
  - `{{node_id.output}}`: full output from `node_id` (truncated to `output_inject_max_chars`, default `2000`)
  - `{{node_id.status}}`: `passed`/`failed`
  - `{{node_id.stdout_first_line}}`: first line (truncated)
  - `{{node_id.stdout_first_token}}`: first token of first line (e.g. `git log --oneline` hash)

### Release-check templates

The former `release_ready_check` presets (fast / strict) went away with the aggregate tools; use `run_command`:

- Fast local loop (quality gates only):
  ```json
  {"command":"bash","args":["-c","cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings"]}
  ```

- Before a release (gates + dependency security + clean worktree):
  ```json
  {"command":"bash","args":["-c","cargo test && cargo audit && cargo deny check advisories licenses bans sources && git status --porcelain"]}
  ```


Also: `cargo_check`, `cargo_test` (result cache when enabled), `cargo_clippy`, rust-analyzer tools, `read_binary_meta`, `code_nav`, `markdown_check_links`, `structured_*`, `table_text`, `text_diff`, `diagnostic_summary`.

**Python**: `pytest_run`, `python_snippet_run` (`ruff` / `mypy` / `uv` / `pre-commit` now via `run_command`, see above). The former `run_lints` aggregator was removed on 2026-10-28 — call `run_command` directly (`cargo clippy --all-targets --all-features`, `npm run lint`, `ruff check .`).

**JVM / Container / Node frontend**: wrappers removed per §8.5.4 **track A**; use `run_command` (`mvn` / `gradle` / `docker` / `podman` / `npm` / `npx` / `tsc` are allowlisted). The JVM implementation was deleted; container / frontend commands are now invoked through `run_command` directly.

**`cargo_clean`**: removed per the thin-CLI-wrapper consolidation (see [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.1.1 pilot); use `run_command` invoking `cargo clean` (`cargo` is allowlisted). Note: after this downgrade there is **no** `dry_run`-defaults-to-true safety default — pass `--dry-run` explicitly to preview only.

**Write-side safety-gate family (§3.1.2)**: `cargo_fix` / `go_mod_tidy` / `python_install_editable` were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.1.2; use `run_command` (`cargo` / `go` / `uv` / `python3` are allowlisted). All three are write-side-effect tools, so the downgrade has **no** read-only-semantics loss; their former `confirm` gate no longer applies, but that gate was **not** a hard security boundary anyway (an allowlisted `run_command` is not subject to sub-command-level approval) — for stronger guarantees use `tool_approval` or a workflow `requires_approval`.

**cargo metadata / bypass family (§3.2)**: seven read-only thin wrappers — `cargo_metadata` / `cargo_tree` / `cargo_doc` / `cargo_outdated` / `cargo_machete` / `cargo_udeps` / `cargo_publish_dry_run` — were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`cargo` is allowlisted). Cost is per §2.1: `run_command` is a write-effect tool, so the downgrade **loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache** (the parallel read-only batch never applied, since the `cargo_` prefix is already excluded); `/health` no longer probes `cargo-machete` / `cargo-udeps`.

**Go (§3.2)**: the five read-only thin wrappers `go_build` / `go_test` / `go_vet` / `go_fmt_check` / `golangci_lint` were removed as a family per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`go` / `gofmt` / `golangci-lint` are allowlisted; `./...` passes the arg-safety check).

**`rustc` direct compile (§3.2)**: `rust_rustc` (invoke `rustc` at workspace root with the same arg safety rules as `run_command`) was removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`rustc` is allowlisted). Cost is per §2.1: the downgrade **loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache** (the `rustc` prefix was never in the parallel read-only batch).

**Process/port (§3.2)**: the two read-only thin wrappers `port_check` / `process_list` were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`ss` / `lsof` / `ps` are allowlisted). Cost is per §2.1: the downgrade **loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache** (the `port_check` / `process_list` prefixes were never in the parallel read-only batch).

**Package info (§3.2)**: the read-only thin wrapper `package_query` was removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`dpkg-query` / `rpm` are allowlisted). Cost is per §2.1: the downgrade **loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache**, and no longer provides the apt/rpm abstraction or structured JSON output.

**Spell / AST search (§3.2, allowlist-first removal)**: the four thin wrappers `typos_check` / `codespell_check` / `ast_grep_run` / `ast_grep_rewrite` were removed per [tool_thin_wrapper_candidates.md](../design/tool_thin_wrapper_candidates.md) §3.2; use `run_command` (`typos` / `codespell` / `ast-grep` are allowlisted; `sg` is **not**, avoiding the system switch-group utility). The typos/codespell **function bodies are kept** for `docs_health_sweep` reuse. Cost is per §2.1: the downgrade **loses read-only retry / `SessionMode::Ask`·`Plan` read-only gating / read-only TTL cache**, plus the path-safety and existence checks; `ast_grep_rewrite`'s write-disk `confirm` gate no longer applies (`ast-grep run --rewrite` needs an explicit `--update-all`).

**Background jobs**: `background_job` (`action` = `status` / `list` / `output`, all read-only; `status` queries status/output by `tool_job_id`, `list` lists workspace jobs newest first with optional `limit`, `output` pulls incremental output with `tail -f` semantics and an optional `cursor`), `background_job_cancel` (cancel a job; write side effect, so it keeps its own tool name).

**Git**: read-only queries are one tool, **`git`**, with a required `action` selector (`status` / `diff` / `clean_check` / `log` / `show` / `blame` / `file_history` / `branch_list` / `remote_status` / `remote_list`; `action=diff` absorbed the former `git_diff_stat` / `git_diff_names` / `git_diff_base` via `stat` / `name_only` / `base`). Write operations (`git_checkout`, `git_branch_create`/`git_branch_delete`, `git_push`, `git_merge`, `git_rebase`, `git_stash`, `git_tag`, `git_reset`, `git_cherry_pick`, `git_revert`, `git_commit`, `git_stage_files`, `git_fetch`, `git_clone`, `git_remote_set_url`, `git_apply`, …) were removed per §8.5.4 **track B**; invoke `git` via `run_command` (allowlisted) instead.

**Metrics**: `code_stats`, `dependency_graph`, `coverage_report`.

**Static analysis / lint CLIs**: `shellcheck`, `cppcheck`, `semgrep`, `hadolint`, `bandit`, `lizard` are on the default `run_command` allowlist and are invoked via generic `run_command` (the dedicated `shellcheck_check` / `cppcheck_analyze` / `semgrep_scan` / `hadolint_check` / `bandit_scan` / `lizard_complexity` tools were removed). Install hints for missing CLIs now come from the model reading `run_command`'s failure output (the former `error_output_playbook` was removed on 2026-10-27).

### Python & pre-commit examples

> `ruff_check` / `mypy_check` / `uv_sync` / `uv_run` are consolidated into **`run_command`** (e.g. `ruff check .` / `mypy src` / `uv sync` / `uv run pytest -q`); no dedicated tool or JSON args remain. Likewise, `pre_commit_run` is consolidated into **`run_command`** (e.g. `pre-commit run --all-files`).

- `pytest_run`:
  ```json
  {"test_path":"tests","quiet":true}
  ```

## Git tool examples

Read-only Git queries are one tool, `git`, selected by a required `action` (`status` / `diff` / `clean_check` / `log` / `show` / `blame` / `file_history` / `branch_list` / `remote_status` / `remote_list`). Each action reads only its own field group; extra fields are ignored. `action=diff` absorbed the former `git_diff_stat` / `git_diff_names` / `git_diff_base` via `stat` / `name_only` / `base`:

- `clean_check`:
  ```json
  {"action":"clean_check"}
  ```
- `diff` (working-tree stat, equivalent to the old `git_diff_stat`):
  ```json
  {"action":"diff","mode":"working","stat":true}
  ```
- `diff` (changed file names vs `main`, equivalent to the old `git_diff_names` / `git_diff_base`):
  ```json
  {"action":"diff","mode":"all","base":"main","name_only":true}
  ```
- `show` (default HEAD):
  ```json
  {"action":"show","rev":"HEAD~1"}
  ```
- `blame` / `file_history` (`path` required):
  ```json
  {"action":"file_history","path":"src/main.rs","max_count":10}
  ```
- `remote_list`:
  ```json
  {"action":"remote_list"}
  ```

Write operations (`add` / `commit` / `push` / `fetch` / `checkout` / `apply` / `clone`, …) were removed per §8.5.4 **track B**; invoke `git` via `run_command` (allowlisted):

- Stage and commit:
  ```json
  {"command":"git","args":["commit","-am","fix: ..."]}
  ```
- Fetch remote updates:
  ```json
  {"command":"git","args":["fetch","origin","main","--prune"]}
  ```
- Check a patch applies (drop `--check` to actually apply):
  ```json
  {"command":"git","args":["apply","--check","patches/fix.diff"]}
  ```

## Common failure handling

Typical `run_command` invocations of `cargo audit` / `cargo deny check` issues:

- `cargo deny check` (license/policy):
  ```bash
  cargo deny check advisories licenses bans sources
  ```
  Causes: `bans` hits, license allowlist, `sources` policy. Fix: upgrade/replace deps; adjust `deny.toml` with review; time-bound exceptions.

- `cargo audit` (known vulnerabilities):
  ```bash
  cargo audit
  ```
  Causes: RustSec advisories, stale lockfile. Fix: `cargo update` / targeted updates; if upstream unfixed, mitigate; re-run audit + tests.

- Dirty tree (`require_clean_worktree=true`): `git` with `action=clean_check` fails—inspect `git status` / `git diff`; commit, stash, or set `require_clean_worktree=false` for quick local runs.

- Missing tools:
  ```bash
  cargo install cargo-deny
  cargo install cargo-audit
  ```

### Suggested pre-release order

1. Quality gates via `run_command` (`cargo fmt --all --check` + `cargo clippy --all-targets --all-features -- -D warnings`).
2. Full `cargo test`, then `cargo audit` / `cargo deny check` after fixes.
3. Clean tree (`git status --porcelain`) + key tests.
4. Tag / release workflow.

## File/dir helper examples

- `read_binary_meta` (size, mtime, **prefix SHA256**, no full read):
  ```json
  {"path":"assets/app.bin","prefix_hash_bytes":8192}
  ```
  `prefix_hash_bytes` `0` skips hash; max 262144.
- `read_file` (line-streamed, default 500 lines/request):
  ```json
  {"path":"src/main.rs","start_line":1,"max_lines":200}
  ```
  Use **`encoding`** for legacy/BOM/unknown: `gb18030`, `utf-8-sig`, `auto`, …. Default **UTF-8 strict** (errors on invalid bytes). After search hits, prefer **`anchor_line`** + optional **`context_lines`** (default ±120 lines each side, capped by **`max_lines`**) instead of hand-calculated ranges; **do not** combine with `start_line`/`end_line`. Response hints next `start_line`. `"count_total_lines": true` rescans for totals. `start_line`+`end_line` still respect `max_lines`. On **success**, the **first line** is a single JSON object **`{"kind":"crabmate_tool_output","tool":"read_file","version":1,…}`** (machine-readable summary, including **`revision`** — a content SHA-256 snapshot ticket such as `sha256:<hex>` that can be passed to `modify_file` as **`base_revision`**); the legacy human-readable block follows. Failures use stable **`error_code`** values (e.g. `read_file_workspace_*`, `read_file_invalid_range`) in the `crabmate_tool` envelope.
- `modify_file` (**prefer `search_replace`** for localized content edits — text-anchored, no line numbers; use **`replace_lines`** for line-range edits: 1-based inclusive line range + `content`, streamed; inverted start/end lines are swapped; **`dry_run=true`** returns a diff preview without writing):
  ```json
  {"path":"src/huge.rs","mode":"replace_lines","start_line":120,"end_line":135,"content":"// new chunk\n"}
  ```
  If `mode` is omitted but `start_line` / `end_line` is present, CrabMate uses `replace_lines`; if `after_line` is present, it uses `insert_after_line`. Insert example:
  ```json
  {"path":"src/huge.rs","mode":"insert_after_line","after_line":120,"content":"// inserted chunk\n"}
  ```
  **`after_line=0`** inserts at the beginning of the file. **`mode=full` (default) or `mode=overwrite`** (same semantics; `overwrite` stresses whole-file replacement): `content` is the **entire** new file body. Risky shrinks / mass line deletion / clearing a non-empty file require **`confirm_full_overwrite=true`** before writing; rejected writes include a unified diff preview. For localized edits, prefer `search_replace` (text-anchored, no line numbers), or `replace_lines` / `insert_after_line` by line. Valid `mode` values: **`full`**, **`overwrite`**, **`replace_lines`**, **`insert_after_line`**.
  **Revision (CAS) guard — automatic by default**: the **`revision`** in a successful `read_file` header (`sha256:<hex>`) is a content snapshot ticket. Within the same turn CrabMate binds the latest successful read per canonical path; **line-based modes** (`replace_lines` / `insert_after_line` / `edits`) compare the file's current hash against that binding before writing, with **no explicit argument required**. If the file changed, the call fails with **`错误[stale_revision]`** and writes nothing, preventing multiple tool calls in one assistant message from editing by stale line numbers. `mode=full/overwrite` does not depend on line numbers and is not auto-bound.
  - You may still pass **`base_revision`** explicitly (works with every `mode` and with `edits`): an explicit value takes precedence over the turn binding and is **always** checked.
  - To deliberately skip the turn-level auto check, pass **`allow_stale_revision=true`** (an explicit `base_revision` is still checked).
  - **Atomic writes**: `full` / `overwrite` and `edits` write a sibling temp file, `fsync` it, then **atomically rename** it over the target (same as `replace_lines` / `insert_after_line`); the revision check runs immediately before the rename, so concurrent readers never observe a truncated half-written file. Files with multiple hard links (`nlink > 1`) fall back to writing back into the original inode to preserve the other links (that path is not atomic).
- `read_dir`:
  ```json
  {"path":"src","max_entries":50,"include_hidden":false}
  ```
- `markdown_check_links`:
  ```json
  {"roots":["README.md","docs"],"max_files":300,"check_fragments":true,"output_format":"text","allowed_external_prefixes":["https://example.com/docs/"],"external_timeout_secs":10}
  ```
  Default roots: `README.md` and `docs/`.
- `structured` (dispatch on **`action`**; minimal payloads below):
  - `action=validate`:
  ```json
  {"action":"validate","path":"frontend/Cargo.toml","format":"auto","summarize":true}
  ```
  - `action=query`:
  ```json
  {"action":"query","path":"Cargo.toml","query":"/package/name","format":"toml"}
  ```
  Dot path: `{"action":"query","path":"frontend/Cargo.toml","query":"dependencies.leptos"}`.
  - `action=diff`:
  ```json
  {"action":"diff","path_a":"specs/openapi.v1.json","path_b":"specs/openapi.v2.json","max_diff_lines":200}
  ```
  **CSV/TSV**: `format` `csv`/`tsv` or by extension; `has_header=false` → array-of-arrays; query example `{"action":"query","path":"data/sample.csv","query":"/0/name"}`.
  - `action=patch` (JSON/YAML/TOML, key-path `set`/`remove`):
  ```json
  {"action":"patch","path":"config/app.toml","query":"/llm/model","value":"glm-5","dry_run":false}
  ```
  `mutation` defaults to `set` (`remove` deletes the path); `create_missing` defaults to `true`; omitted `dry_run` previews only. Fields belonging to other actions are ignored; `format=csv|tsv` is rejected for `action=patch`.
- `table_text`: preview `{"action":"preview","path":"data/foo.tsv","preview_rows":15}`; filter `{"action":"filter_rows","path":"data/foo.csv","column":2,"contains":"ERR","max_output_rows":100}`.
- `diagnostic_summary` (**no env values**):
  ```json
  {"include_toolchain":true,"include_workspace_paths":true,"include_env":true,"extra_env_vars":["CI"]}
  ```
- `search_in_files` (regex content search; `.gitignore`-aware walk):
  ```json
  {"pattern":"TODO|FIXME","path":"src","max_results":50,"case_insensitive":true}
  ```
  On success, line 1 is **`crabmate_tool_output` v1** (`pattern`, workspace-relative `root`, `match_count`, `files_visited`, `truncated`, …). Invalid regex → `search_in_files_invalid_regex`; bad glob → `search_in_files_invalid_glob`.
- `codebase_semantic_search` (fastembed/ONNX; same stack as long-term memory):
  - Workspace-wide incremental rebuild (query optional):
  ```json
  {"rebuild_index": true}
  ```
  - Force full rebuild:
  ```json
  {"rebuild_index": true, "incremental": false}
  ```
  - Rebuild subtree only:
  ```json
  {"rebuild_index": true, "path": "src"}
  ```
  - Default **hybrid** query:
  ```json
  {"query": "chat job queue concurrency", "top_k": 6, "query_max_chunks": 20000}
  ```
  - Vector-only / FTS-only:
  ```json
  {"query": "How does SSE approval work?", "retrieve_mode": "semantic_only", "top_k": 8}
  ```
  ```json
  {"query": "hybrid_alpha", "retrieve_mode": "fts_only", "top_k": 10, "fts_top_n": 200}
  ```
- `apply_patch` (**unified diff**; dry-run then apply—small steps, rollback-friendly, context-rich):
  - Same format as `git diff`: `---`/`+++` headers, `@@` hunks, `-`/`+` lines, **context lines start with a single space**.
  - Keep **2–3** context lines around edits; avoid zero-context one-line hunks.
  - **Paths & strip**: prefer `--- src/file.rs` / `+++ src/file.rs` with default **`strip` 0**; git-style `a/` `b/` paths need **`"strip": 1`**. No `..` in paths.
  ```text
  --- src/example.rs
  +++ src/example.rs
  @@ -4,6 +4,7 @@ fn demo() {
       let a = 1;
       let b = 2;
  -    let c = a + b;
  +    let c = a + b + 1;
       println!("{}", c);
       // trailing context
  ```
  ```json
  {"patch":"--- src/example.rs\n+++ src/example.rs\n@@ -4,6 +4,7 @@ fn demo() {\n ...\n"}
  ```
- `code_nav` (Rust code navigation; `action` picks the verb — the four tools `find_symbol` / `find_references` / `rust_file_outline` / `call_graph_sketch` were merged into it on 2026-10-25):
  ```json
  {"action":"find_symbol","symbol":"run_agent_turn","kind":"fn","path":"src","context_lines":2,"max_results":10}
  ```
  ```json
  {"action":"find_references","symbol":"run_tool","path":"src/cm_tools/tools","max_results":50,"case_sensitive":false,"exclude_definitions":true}
  ```
  ```json
  {"action":"call_graph_sketch","symbols":["run_tool_dispatch","RunAgentTurnParams"],"path":"src","max_edges":600}
  ```
  ```json
  {"action":"file_outline","path":"src/cm_tools/mod.rs","include_use":false,"max_items":200}
  ```
  **Required**: `find_symbol` / `find_references` / `call_graph_sketch` need `symbol` (`call_graph_sketch` also accepts `symbols`), `file_outline` needs `path`; fields unrelated to the chosen `action` are ignored (unknown keys still fail), though each call still has to satisfy the schema ranges (e.g. `max_results` 1–300). `call_graph_sketch` is a **heuristic** impact checklist (`use` + non-definition word-boundary hits grouped by directory; not a real call graph). Use `rust_analyzer` when you need LSP-grade precision.
