**Languages / 语言:** [中文](../命令行与路由.md) · English (this page)

# CLI and subcommands

Help: `crabmate --help`, `crabmate help`, `crabmate help <subcommand>` (same as `--help`). Root and **`chat --help`** footers cross-reference **`docs/命令行契约.md`** and **`docs/SSE协议.md`**. **Global options** go **before** the subcommand: `--config`, `--workspace`, `--agent-role`, `--no-tools`, `--log`.

**Script contract** (exit codes, `chat --output json` line JSON `type`/`v`, etc.): [`CLI_CONTRACT.md`](CLI_CONTRACT.md).

## Man page (troff / `man`)

- **Source tree**: Pre-generated **`man/crabmate.1`** (troff), aligned with current `clap`; **Debian `.deb`** installs to **`/usr/share/man/man1/crabmate.1`** (see root `Cargo.toml` `[package.metadata.deb] assets`).
- **Regenerate** (after adding/removing subcommands or global flags): `cargo run --bin crabmate-gen-man`, then commit the updated `man/crabmate.1`.
- **`cargo install`**: Does **not** install man into `MANPATH` by default; copy `man/crabmate.1` to `.../share/man/man1/` and run `mandb` (distro-dependent), or prefer **`cargo deb`** / distro packages.

## Subcommand overview

| Subcommand | Description |
|------------|-------------|
| `serve [PORT]` | Web UI + HTTP API, default **8080**; with **`bearer`**, may **start without `API_KEY`**; set the **LLM** key in sidebar **Settings** (`client_llm`) before chatting. When **`web_api_bearer_token`** / **`CM_WEB_API_BEARER_TOKEN`** is set, also save the **same** shared secret under **Settings → Web API shared secret** (not the LLM key). **Temporary skip**: `unset` the secret and bind **`127.0.0.1`**, or clear it and set **`CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`** before **`0.0.0.0`** (see **`docs/en/CONFIGURATION.md`**). **Desktop Tauri** is a thin client: start **`serve`** yourself, then connect from the shell (see Client repo **`../crabmate-client/desktop-tauri/DEVELOPMENT.md`**). |
| `repl` | Interactive chat; **default when no subcommand**. With **`bearer`** and no env **`API_KEY`**, use **`/api-key set <secret>`** before sending messages. |
| `tui` | Full-screen terminal UI (**experimental**); phase B/C: layout + minimal chat loop sharing **`repl_dispatch_chat_round`** with **`repl`**. **Requires an interactive TTY for stdin and stdout**. Assistant output is **not rendered to stdout** (alternate-screen safe); respects global **`--no-stream`** for SSE vs JSON. **`Enter`** sends the line; **`/api-key`** / **`/apikey`** supported (feedback in the transcript); slash commands match **`repl`**; with **`conversation_store_sqlite_path`** set, **`/conv`** / **`/branch`** manage SQLite sessions like Web. **`q`/`Q` with empty input** or **Ctrl+C** exits. Loads **`AgentConfig`** like **`repl`**. See **`runtime/tui`**. |
| `chat` | One-shot / scripted chat: `--query` / `--stdin` / `--user-prompt-file`, `--system-prompt-file`, `--messages-json-file`, `--message-file` (JSONL), `--yes` / `--approve-commands`, `--output json`, `--no-stream`. With **`bearer`** and no **`API_KEY`**, the first turn fails unless you export **`API_KEY`** or use **`repl`** / **`serve`** as above. |
| `bench` | Batch eval: `--benchmark`, `--batch`, etc. |
| `config` | Config + **`API_KEY`** status self-check; optional `--dry-run`. |
| `doctor` | Local diagnostics (**no** `API_KEY`). |
| `models` | `GET …/models` (needs `API_KEY`). |
| `probe` | Probe models endpoint (needs `API_KEY`). |
| `save-session` | Export JSON/Markdown from session file to workspace **`.crabmate/exports/`** (same shape as Web; **no** `API_KEY`). `--format json|markdown|both` (default `both`), optional `--session-file`. Alias **`export-session`**. |
| `tool-replay` | Extract **tool-call timeline** from session JSON as fixture, or **replay tools** from fixture via `run_tool` (**no** LLM; **no** `API_KEY`). See “Tool replay fixture” below. |
| `mcp list` | Read-only list of in-process MCP sessions from user-data `mcp_servers.json` (stdio / remote) and merged OpenAI tool names (**no** `API_KEY`). Legacy TOML/`CM_MCP_COMMAND` imports once when the file is empty and **`toml_legacy_imported`** is unset. If no chat has run yet, **`mcp list --probe`** tries one connection. |
| `plugin init` | Generate a dynamic tool template under workspace **`plugins/*.json`** (name must use `dyn__` prefix); default output path is `plugins/<name-without-prefix>.json`. |
| `plugin list` | List dynamic tool files, tool names, commands, and validation status (OK/FAIL); use `--json` for structured output, or `--jsonl` for line-oriented pipelines. |
| `plugin validate` | Validate dynamic tool definitions (scan `plugins/*.json` by default, or a single file via `--file`); checks JSON shape and whether `command` is in `allowed_commands`; use `--json` for structured output, or `--jsonl` for line-oriented pipelines. |
| `mcp serve` | Run an **MCP server** on **stdin/stdout** (default) or **TCP port** (`--port N`), exposing CrabMate built-in tools (`tools/list` / `tools/call` → **`tools::run_tool`**; **no** `API_KEY`). Working directory follows global **`--workspace`** / config **`run_command_working_dir`**. JSON-RPC uses **stdout** (stdio mode) or TCP stream; use **stderr** for human messages. **`--no-tools`**: advertise an empty tool list. **`--port`**: TCP port number (default `0` for stdio mode), binds `127.0.0.1`. **No transport auth**: trusted local or SSH-tunnel integration only; same capability as `run_command` allowlist, workspace path rules. TCP mode enables remote development via SSH port forwarding. |

## Log levels

Without `RUST_LOG`: `serve` defaults to **info**; `repl` / `chat` / `tui` / `bench` / `config` / `mcp` / `save-session` (and alias `export-session`) / `tool-replay` / `plugin` default to **warn**. Use `RUST_LOG` or `--log <FILE>`.

## Message pipeline debug logs

With `RUST_LOG=crabmate=debug`, each model call prints **`message_pipeline session_sync`** summary; finer: `RUST_LOG=crabmate::message_pipeline=trace`. Cumulative hits: **`GET /status`**; implementation: `src/agent/message_pipeline/`.

## Legacy usage

Without a subcommand, legacy flags `--serve`, `--query`, `--benchmark`, `--dry-run`, etc. still map internally. If argv **anywhere** contains an explicit subcommand name (`serve`, `doctor`, `tui`, `save-session`, `export-session`, `tool-replay`, `plugin`, …), the default `repl` is **not** inserted (see `tests/fixtures/cli/legacy_normalize.json`).

## Common options (compat)

| Option | Description |
|--------|-------------|
| `--config <path>` | Config file (prefer before subcommand) |
| `--serve [port]` | Same as `serve` |
| `--host <ADDR>` | With `serve` |
| `--port 0` | With `serve`: OS-assigned free port; startup log and **`web_ready`** **`port`/`url`** use **`local_addr()`** after bind |
| `--desktop-ready-json` | With `serve`: after listen succeeds, print one **`web_ready`** JSON line to **stdout** (for scripts/tools; the desktop shell **no longer** depends on it). **Deprecated name**; prefer alias **`--web-ready-json`** (same behavior) |
| `--web-ready-json` | Alias of `--desktop-ready-json` |
| `--query` / `--stdin` | Same as `chat` |
| `--workspace <path>` | Override initial workspace |
| `--agent-role <id>` | First-turn `system` for new `repl` / `chat` session (must exist in config; mutually exclusive with `chat --system-prompt-file`) |
| `--output` | With `chat`: `plain` or `json` |
| `--no-tools` | Disable tools |
| `--no-web` / `--cli-only` | API only |
| `--dry-run` | Maps to `config` |
| `--no-stream` | With `repl` / `chat` |
| `--log <FILE>` | Log file + stderr mirror |

## Benchmark (`bench`)

Planning for benchmark features and testing: **`docs/基准测试规划.md`** (kept separate from general CLI product notes).

| Option | Description |
|--------|-------------|
| `--benchmark <TYPE>` | `swe_bench`, `gaia`, `human_eval`, `generic` |
| `--batch <FILE>` | Input JSONL (`human_eval` rows need `humaneval_test` and `entry_point`; see **`docs/基准测试规划.md`** §5) |
| `--batch-output <FILE>` | Default `benchmark_results.jsonl` |
| `--task-timeout <SECS>` | `0` = no limit |
| `--max-tool-rounds <N>` | `0` = no limit |
| `--resume` | Skip existing `instance_id` |
| `--bench-system-prompt <FILE>` | Override system |

HumanEval: convert the official JSONL, run `bench`, then score with Python 3 (**executes model-generated code** — sandbox if needed):

```bash
python3 scripts/humaneval_official_to_crabmate_jsonl.py --input HumanEval.jsonl --output tasks.jsonl
cargo run -- bench --benchmark human_eval --batch tasks.jsonl --batch-output results.jsonl
python3 scripts/humaneval_score_benchmark_results.py --tasks tasks.jsonl --results results.jsonl
```

## Examples

```bash
cargo run                                    # default repl
cargo run -- --config /path/to/my.toml serve
RUST_LOG=debug cargo run -- --log /tmp/crabmate.log repl
cargo run -- serve
cargo run -- serve 3000
cargo run -- serve --port 3000               # same as above
cargo run -- --workspace /path/to/project serve 8080
cargo run -- serve --host 0.0.0.0            # mind auth & safety
cargo run -- serve --host 127.0.0.1 --port 0 --web-ready-json   # optional: print web_ready for scripts (legacy alias: --desktop-ready-json)
cargo run -- chat --query "What's the weather in Beijing?"
cargo run -- chat --output json --query "…"
echo "1+1?" | cargo run -- chat --stdin
cargo run -- --no-tools serve
cargo run -- bench --benchmark swe_bench --batch tasks.jsonl --batch-output results.jsonl --task-timeout 600
cargo run -- config
cargo run -- save-session
cargo run -- save-session --format json --workspace /path/to/proj
```

## `save-session`

Reads **`<workspace>/.crabmate/tui_session.json`** by default (`--workspace` and global `--config` before subcommand), writes timestamped **`chat_export_*.json`** / **`chat_export_*.md`** under **`<workspace>/.crabmate/exports/`** (schema / Markdown titles shared via **`crabmate-chat-export`**). JSON envelope (**required** fields): `schema` (`crabmate.chat_session`), `schema_version` (currently **`2.0.0`**), **`projection`** (**`raw`**: full OpenAI-shaped `Message` from CLI/TUI/`save-session`/session files; **`display`**: Web/Tauri display projection — **not** valid **`tool-replay`** input), `version`, `messages`. Legacy JSON missing envelope keys is **rejected**. See `crates/crabmate-chat-export`, `runtime/chat_export.rs`, `frontend/src/session_export.rs`. Each stdout line is the absolute path of a written file for scripts.

## `tool-replay` (tool timeline fixture)

Reproduce **tool call order and arguments** from a chat, or **regression-compare** outputs vs recorded `tool` messages.

- **`export`**: Scan **`ChatSessionFile`** (same shape as `save-session` / Web export) for `assistant.tool_calls` and following `role=tool` messages; write **`tool_replay_YYYYMMDD_HHMMSS.json`** to **`.crabmate/exports/`** (or `--output`). Top-level: `version`, `source: "crabmate-tool-replay"`, optional `note`, **`steps`** (`name`, `arguments`, `tool_call_id`, optional **`recorded_output`**).
- **`run`**: For each `step`, call **`tools::run_tool`** on the current workspace (**real** execution: `run_command` / `http_fetch` still obey config and allowlist; **no** terminal approval UI—non-whitelist `run_command` fails). With `--compare-recorded`, string equality vs `recorded_output`; mismatch → exit code **6**.

```bash
crabmate save-session --format json --workspace /path/to/proj   # get chat_export_*.json
crabmate tool-replay export --session-file /path/to/chat_export_20260101_120000.json --note "bug repro"
crabmate tool-replay run --fixture /path/to/proj/.crabmate/exports/tool_replay_20260101_120500.json
crabmate tool-replay run --fixture ./fixture.json --compare-recorded   # CI regression
```

**Safety**: Same trust model as a normal agent turn; use only in **trusted workspaces**; do not run untrusted session fixtures against sensitive directories.

## `chat` and pipes

Exactly one of `--query`, `--stdin`, `--user-prompt-file`. `--system-prompt-file` overrides configured system. `--messages-json-file` supplies full messages for one turn. `--message-file` is JSONL batch.

**Exit codes**: **0** success; **1** general error; **2** usage; **3** model/parse failure; **4** all `run_command` denied this turn; **5** quota/rate-limit style (e.g. 429).

## Built-in CLI commands

**Startup banner**: Interactive CLI prints sections—**model** (truncated `api_base`, `llm_http_auth`, `temperature`, `llm_seed`, current **`--no-stream`**), **workspace & tools**, **slash commands**, **key config** (`max_tokens`, `max_message_history`, API timeouts/retries, `run_command` timeout/output caps, optional session restore/MCP/long-term memory, etc.). Styling matches **`cli_repl_ui`** `/help`; **`NO_COLOR`** or non-TTY disables ANSI. **`/config`** reprints a **key config summary** anytime (same family as banner, **no** secrets).

**Optional**: **`CM_CLI_WAIT_SPINNER=1`** shows stderr spinner and elapsed time while waiting for the **first** streaming chunk (or full body with **`--no-stream`**); default off; needs stderr TTY and no **`NO_COLOR`**. See **`docs/配置说明.md`**.

**SyncDefault Docker (CLI + `chat`)**: Optionally run **SyncDefault** and some tools inside **Docker** after host approval/allowlist (**`sync_default_tool_sandbox_mode = docker`**, image, `user`, etc.; Unix often uses **effective uid:gid** for workspace ownership). Full notes in **`docs/配置说明.md`** § SyncDefault Docker sandbox.

**Feedback style**: Success/error lines start with **✓** / **✗**; with **`NO_COLOR`** or non-TTY use **`[ok]` / `[err]`** (ASCII).

Slash commands: **`/help`**, **`/clear`**, **`/model`**, **`/api-key`** (**`status` / `set <secret>` / `clear`**; in-process LLM Bearer key, not persisted; alias **`/apikey`**), **`/config`** (no args), **`/doctor`** (same as **`crabmate doctor`**), **`/probe`** (same as **`crabmate probe`**), **`/models`** / **`/models list`** (same as **`crabmate models`**), **`/models choose <id>`** (set in-memory **`model`** from latest **`GET …/models`** list, unique case-insensitive prefix; persist via config; **`/config reload`** overwrites from disk), **`/agent`** / **`/agent list`** (list configured role ids, same source as **`GET /status`** **`agent_role_ids`**; prints a hint when multi-role is not configured), **`/agent set <id>`** / **`/agent set default`** (set or clear this REPL’s explicit **`agent_role`**; **replace only the first `system`**, keep the transcript for multi-role workbench), **`/skills`** / **`/skills list`** (merged workspace + user + system skills; **`/<skill-id> [task]`** force-selects one), **`/workspace`** / **`/cd`**, **`/tools`**, **`/export`** (optional `json` / `markdown` / `both`, default `both`; **current memory**), **`/save-session`** (same format args; reads disk **`tui_session.json`**, same as **`crabmate save-session`**). `quit` / `exit` / Ctrl+D exit.

**Tab completion** (interactive TTY, **reedline**): Under the “me:” prompt, if the line before the cursor (trimmed) starts with **`/`**, **Tab** opens slash-command completion (arrows or Tab to select; single match may auto-fill). After **`/export`** or **`/save-session`**, **Tab** completes **`json` / `markdown` / `md` / `both`**. After **`/mcp`**: **`list`**, **`probe`**, **`list probe`**. After **`/models`**: **`list`**, **`choose`** (**`choose`** gets a trailing space for model id). After **`/agent`**: **`list`**, **`set`** (**`set`** gets a trailing space for role id). **`/api-key`** and **`/apikey`** appear in the root completion list. Completion is off in **`bash#:`** local shell mode.

**`/mcp`**: Read-only MCP stdio cache and merged tool names (same as **`crabmate mcp list`**); **`/mcp probe`** or **`/mcp list probe`** tries one connection (starts **`mcp_command`**). **`/version`**: `crabmate` version and **`OS`/`ARCH`** (no secrets).

**`/config reload`**: Re-merge TOML from the startup config path (**`--config`**, or default cwd **`config.toml`** / **`.agent_demo.toml`** then **`$XDG_CONFIG_HOME/crabmate/config.toml`**) with current env into memory **`AgentConfig`**—**`api_base`**, model, timeouts, allowlists, MCP, **re-read `system_prompt_file`**, etc.; **does not** reopen session SQLite or rebuild shared **`reqwest::Client`**; **does not** re-read env **`API_KEY`** (REPL **`/api-key`** memory is **not** cleared). Web keys still come from **`client_llm.api_key`** or process **`API_KEY`** at startup. Web equivalent: **`POST /config/reload`**. If Bearer middleware was enabled at startup, toggling token still needs **`serve` restart**. See **`docs/配置说明.md`** § Hot reload.

**Tool stdout**: After each tool in interactive CLI / **`chat`** (no SSE), prints **`### Tool · …`** title and body. **`read_file`**, **`read_dir`**, and **`list_tree`** print a **terminal summary** (headers + first N lines of content; lines may be truncated) and note that the full output is in history; other tools print the body (truncated if over limit). Full tool results stay in history for the model. On **failure** (non-zero `run_command`, `错误：` / error-prefix style messages, etc.), terminal may print a **self-heal hint · diagnostic command bundle**: one JSON line for the model to call **`playbook_run_commands`** (same heuristics as **`error_output_playbook`**, but **executes** allowlisted `run_command`; **sanitize** `error_text` first). Commands are **not** auto-run.

### Leading `$` (local shell boundary)

On **interactive TTY**, when the input buffer is **empty**, **`$`** (or fullwidth **`＄`**) **without Enter** toggles between “me:” and **`bash#:`**; still supports a line that is only **`$`** then Enter. In **`bash#:`**, one line runs as **local shell** via **`sh -c`** (Windows **`cmd /C`**) in the current workspace directory—**not** the model, **not** `run_command` allowlist—same as typing in your own terminal (any `sh -c` program; stdin cleared). If the line already has text, **`$` inserts normally** (e.g. dollar amounts). **Trusted machine / workspace only**; for controlled commands, use the model with `run_command`. Pipes/non-TTY: inline **`$ <cmd>`**. TTY history: **`.crabmate/repl_history.txt`** in the workspace (separate from model session file).

On model/network failure, interactive CLI prints error and **continues**; use **`/clear`** if history is inconsistent (keeps current `system`).

## `run_command` terminal approval

If the command is not allowlisted: when **stdin** and **stderr** are TTY, **stderr** shows a **dialoguer** menu (arrows; **`NO_COLOR`** plain theme); otherwise **non-interactive**: print instructions, read one line—**y** once; **a** / **always** allow this command name for the session; **n** / Enter deny (good for `echo y` in CI). **`chat --yes`** auto-approves non-whitelist **`run_command`** and unmatched-prefix **`http_fetch` / `http_request`** (very dangerous). **`chat --approve-commands a,b`** adds extra allowed **command names** only (not HTTP URLs).

## CLI vs Web (persistence / approval / export)

| Capability | Web (`serve`) | CLI |
|------------|---------------|-----|
| **Session persistence** | Embedded default: SQLite at **`.crabmate/conversations.db`** (`conversation_store_sqlite_path`) + `conversation_id`, multi-session, survives restart (TTL/limits; see `docs/开发文档.md`); clear the path for in-memory-only. | **Partial**: **`repl`** optional load/save **`.crabmate/tui_session.json`** (`tui_load_session_on_start` / `tui_session_max_messages`), **single** chain file. **`tui`** uses the same SQLite session DB as Web when **`conversation_store_sqlite_path`** is set (`CM_TUI_CONVERSATION_ID` optional; **`/conv`** / **`/branch`**); then **`tui_session.json`** is **not** written on exit. **`tui_session.json`** remains for **`tui`** without session SQLite. `chat` does not persist across invocations by default; use `--messages-json-file`, etc. **`repl_initial_workspace_messages_enabled`** (default false; see `docs/配置说明.md`): when true, CLI builds **`initial_workspace_messages`** in background (profile, deps, disk restore); when false, startup is one `system` only—no tokei / `cargo metadata` on boot. |
| **Human approval** | Non-whitelist `run_command`, **`http_fetch` / `http_request`** without `http_fetch_allowed_prefixes` match: SSE control plane + **`POST /chat/approval`** (non-stream `/chat` without approval session may reject). | **`run_command`**: see above (TTY menu / pipe). **`http_fetch` / `http_request`**: same approval; permanent key for **`http_request:<METHOD>:<URL>`** vs **`http_fetch:`**. |
| **Export chat** | Frontend export JSON/Markdown (shape aligned with `.crabmate/tui_session.json`; see `README.md`). | **`save-session`** (alias **`export-session`**) from disk session → **`.crabmate/exports/`**; interactive **`/save-session`** same; **`/export`** exports **in-memory** messages. `chat --output json` is **not** full session export. |

Keep this section in sync with `README.md` when export behavior changes.

## Frontend build and Web

```bash
cd ../crabmate-client && make frontend
export CM_WEB_STATIC_DIR="$PWD/frontend/dist"
cd ../crabmate_agent && cargo run -- serve
```

Pure API: `cargo run -- serve --no-web`. Config check: `cargo run -- --dry-run --no-web`.

Static assets come from `CM_WEB_STATIC_DIR` (or sibling Client `frontend/dist`).

## Main HTTP routes (`serve`)

### HTTP auth matrix

Matches **`src/web/server.rs`**. When **`web_api_bearer_token` / `CM_WEB_API_BEARER_TOKEN`** is non-empty at startup, protected routes get the Bearer layer (`Authorization: Bearer` **or** `X-API-Key`). Empty secret → no layer (trusted environments only).

| Class | Paths | Bearer layer | Notes |
|-------|-------|--------------|-------|
| Protected API | `/chat*`, `/conversation/*`, `/workspace*`, `/skills`, `/tasks`, `/github/*`, `/config/*`, `/user-data/*`, `/upload`, `/uploads/delete` | **Yes** (if token non-empty at start) | See configuration docs |
| Public system | `GET /health`, `GET /status` | **No** | **Not** behind Bearer even when configured; isolate via bind/`127.0.0.1`/firewall/proxy |
| Spec / shell | `GET /openapi.json`, `GET /web-ui` | **No** | |
| Static | `/`, SPA, `/uploads/*` files | **No** | |
| E2E | `/e2e/...` | Conditional | **`CM_E2E_FIXTURES=1` only** |

Every `build_app` response includes **`x-request-id`** (echo inbound if valid, else generate). Prefer the response header for correlation; **4xx/5xx** JSON **`ApiError.request_id`** matches the header (middleware fills when missing). See **`docs/en/CLI_CONTRACT.md`**.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/openapi.json` | OpenAPI 3.0 spec (`application/json`); aligns with routes below; SSE line semantics in **`docs/SSE协议.md`** |
| GET | `/` | Frontend |
| POST | `/config/reload` | Hot-reload in-memory `AgentConfig` (not SQLite path); body `{}` ok; see **`docs/配置说明.md`** |
| POST | `/config/session/conversation-store` | Switch Web session backing for **this `serve` process** only: **`{"sqlite":true}`** opens SQLite from configured **`conversation_store_sqlite_path`**; **`{"sqlite":false}`** uses **in-memory** storage (**does not** rewrite config files; **restart `serve`** still follows TOML). Same auth as **`/config/reload`**. Success body **`ok`**, **`message`**; failure **400** `SESSION_STORE_SWITCH_FAILED` (e.g. SQLite requested but path empty). |
| POST | `/chat` | JSON chat; optional `conversation_id`, `agent_role` (new server-side session only), `temperature`, `seed`, `seed_policy` |
| POST | `/chat/stream` | SSE; each event has **`id:`**; headers **`x-conversation-id`**, **`x-stream-job-id`**; optional JSON **`stream_resume`** (`job_id`, `after_seq`) and **`Last-Event-ID`** for reconnect; **410** `STREAM_JOB_GONE` if the job is gone; optional `approval_session_id`, `agent_role` (same) |
| POST | `/chat/approval` | Approval: `approval_session_id`, `decision` |
| POST | `/chat/branch` | Branch/truncate: JSON `conversation_id`, `before_user_ordinal` (0-based plain user index), `expected_revision`; server truncates **before** that user message (same as Web “regenerate from here”: resend the user text via `/chat/stream`). Requires persisted conversation and matching `revision` |
| POST | `/upload` | Multipart upload (protected); returns file URL list (`UploadResponseBody`) |
| POST | `/uploads/delete` | Delete uploaded files by URL list; JSON **`urls`** (only `/uploads/<filename>` shapes) |
| GET | `/tasks` | Sidebar tasks for current workspace (**in-process memory**, not on-disk workspace files); protected |
| POST | `/tasks` | Replace task list and echo (refreshes **`updated_at`**); protected |
| GET | `/status` | Backend status |
| GET | `/workspace` | Workspace list |
| POST | `/workspace` | Set Web workspace root: JSON `{"path":"/abs/dir"}` or project-pool mode `{"project":"my-app"}`; omit `path` or use empty string to reset to default (`run_command_working_dir`); path must exist and lie under `workspace_allowed_roots` |
| GET | `/workspace/projects` | Whether the project pool is enabled and the list of project names (`enabled`, `pool_path`, `projects`); `enabled=false` when `web_workspace_pool` is unset |
| POST | `/workspace/projects` | Open or create a named project workspace: JSON `{"name":"my-app","create":true}`; on success switches the current session workspace and returns `path` |
| GET | `/workspace/pick` | Legacy stub: always `{"path":null}`; Web **File** menu opens the project picker when `web_workspace_pool` is set, otherwise browser `prompt` for an absolute path; Tauri uses a native folder dialog |
| GET | `/workspace/profile` | Project profile Markdown |
| GET | `/workspace/changelog` | Session workspace changelist Markdown (optional `conversation_id` query; same body as **`session_workspace_changelist`** model injection, read-only) |
| POST | `/workspace/search` | In-workspace text search; JSON **`pattern`** (required), optional **`path`**, **`max_results`**, **`case_insensitive`**, **`ignore_hidden`** (see OpenAPI **`WorkspaceSearchBody`**); response **`output`**, may stay **200** with **`error`** on tool failure |
| GET | `/workspace/file` | Read file in workspace (`path` required; optional **`encoding`**, same as `read_file`, default UTF-8 strict; 1 MiB cap) |
| POST | `/workspace/file` | Write file (JSON `path`, `content`; optional **`create_only`** / **`update_only`**) |
| DELETE | `/workspace/file` | Delete file (`path` required; not directories) |
| POST | `/workspace/dir` | Create dir (JSON **`path`**, optional **`parents`**); or delete dir (JSON **`delete=true`**, **`confirm=true`**, optional **`recursive=true`**, same as **`DELETE`**; frontend falls back to **`POST`** on 404/405) |
| DELETE | `/workspace/dir` | Delete directory (`path` query required; **`confirm=true`** required; non-empty dirs need **`recursive=true`**) |
| GET | `/health` | Health |

SSE control-plane fields: **`docs/SSE协议.md`**.

## One-shot packaging (tar.gz + optional .deb)

```bash
./scripts/package-release.sh
```

Artifacts land in **`dist/`** at the repo root: a **`tar.gz`** is always produced (unless **`--skip-tar`**); **`.deb`** is copied from **`target/debian/`** only on **Linux** with **`cargo-deb`** installed (use **`--skip-deb`** to skip). Run **`--help`** for all flags.

## Debian `.deb` package

```bash
cargo install cargo-deb
cargo build --release
cargo deb
sudo dpkg -i target/debian/crabmate_*.deb
```

Server `.deb` does not require UI. To bundle UI in a tarball, build Client dist then run `./scripts/package-release.sh --frontend-dist …/frontend/dist`.

After install: `export API_KEY=… && crabmate serve --no-web` (or set **`CM_WEB_STATIC_DIR`**). Package includes **`/usr/share/man/man1/crabmate.1`** (`man crabmate` if **`MANPATH`** includes `/usr/share/man`).

Preview from tree: `man -l man/crabmate.1` (path relative to repo root).
