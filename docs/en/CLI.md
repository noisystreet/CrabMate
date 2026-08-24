**Languages / 语言:** [中文](../命令行与路由.md) · English (this page)

# CLI and subcommands

Help: `crabmate --help`, `crabmate help`, `crabmate help <subcommand>` (same as `--help`). Root **after_help** cross-references **`docs/命令行契约.md`** and **`docs/SSE协议.md`**. **Global options** go **before** the subcommand: `--config`, `--workspace`, `--no-tools`, `--llm-context-tokens`, `--log`. An explicit subcommand is required (e.g. `serve`); bare `cargo run` no longer defaults into dialogue.

**Official terminal**: Client **`crabmate-tui`** (HTTP/SSE to this repo’s **`serve`**; LLM keys on the client). In-process **`chat` / `repl` / `tui` are hard-deleted** (D2.1 entry + D2.2 implementation—[`design/client_shell_split.md`](../design/client_shell_split.md) §2.5).

**Script contract** (exit codes, `tool-replay`; legacy `chat --output json` shape is historical only): [`CLI_CONTRACT.md`](CLI_CONTRACT.md).

## Man page (troff / `man`)

- **Source tree**: Pre-generated **`man/crabmate.1`** (troff), aligned with current `clap`; **Debian `.deb`** installs to **`/usr/share/man/man1/crabmate.1`** (see root `Cargo.toml` `[package.metadata.deb] assets`).
- **Regenerate** (after adding/removing subcommands or global flags): `cargo run --features gen-man --bin crabmate-gen-man`, then commit the updated `man/crabmate.1`.
- **`cargo install crabmate`** (crates.io **stable `0.5.0`**, default **`server`**; git tag **`v0.5.0`** matches this package): does **not** install man into `MANPATH` by default; copy `man/crabmate.1` to `.../share/man/man1/` and run `mandb` (distro-dependent), or prefer **`cargo deb`** / distro packages.

## Subcommand overview

| Subcommand | Description |
|------------|-------------|
| `serve [PORT]` | HTTP API (API-only by default, port **8080**); host SPA with **`--with-web`** + **`CM_WEB_STATIC_DIR`**. With **`bearer`**, may **start without `API_KEY`**; set the **LLM** key in sidebar **Settings** (`client_llm`, authority on the Client) before chatting. When **`web_api_bearer_token`** / **`CM_WEB_API_BEARER_TOKEN`** is set, also save the **same** shared secret under **Settings → Web API shared secret** (not the LLM key). **Temporary skip**: `unset` the secret and bind **`127.0.0.1`**, or clear it and set **`CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`** before **`0.0.0.0`** (see **`docs/en/CONFIGURATION.md`**). **Desktop Tauri** is a thin client: start **`serve`** yourself, then connect from the shell (see Client [`desktop-tauri/DEVELOPMENT.md`](https://github.com/noisystreet/crabmate-client/blob/main/desktop-tauri/DEVELOPMENT.md)). |
| `bench` | Batch eval: `--benchmark`, `--batch`, etc. |
| `config` | Config + **`API_KEY`** status self-check; optional `--dry-run`. |
| `doctor` | Local diagnostics (**no** `API_KEY`). |
| `web-bearer` | **`status` / `set` / `clear`**: system keyring Web API shared secret (same slot as Web **`/user-data/secrets/web-api-bearer`**; **no** `API_KEY`). Prefer **`set --stdin`**, **`set --from-env`** (reads **`CM_WEB_API_BEARER_TOKEN`**), or interactive hidden prompt (no args); a positional `TOKEN` lands in shell history/`ps` (compat only). When TOML / **`CM_WEB_API_BEARER_TOKEN`** are empty, **`serve`** loads from here; toggling empty↔non-empty still requires a **`serve` restart** to mount/unmount the auth middleware. Browsers must still save the **same** string under **Settings → Web API shared secret**. |
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

Without `RUST_LOG`: `serve` defaults to **info**; `bench` / `config` / `mcp` / `web-bearer` / `save-session` (and alias `export-session`) / `tool-replay` / `plugin` / `workflow` / `doctor` and other subcommands default to **warn**. Use `RUST_LOG` or `--log <FILE>`.

## Message pipeline debug logs

With `RUST_LOG=crabmate=debug`, each model call prints **`message_pipeline session_sync`** summary; finer: `RUST_LOG=crabmate::message_pipeline=trace`. Cumulative hits: **`GET /status`**; implementation: `src/agent/message_pipeline/`.

## Legacy usage

Without a subcommand, legacy **`--serve`**, **`--benchmark`**, **`--dry-run`**, etc. still map internally. Bare argv / **`--query`** / **`--stdin`** are **no longer** mapped to `chat` or default-inserted as `repl` (explicit subcommand required; see `tests/fixtures/cli/legacy_normalize.json`). If argv **anywhere** already contains a known subcommand name, nothing extra is inserted.

## Common options (compat)

| Option | Description |
|--------|-------------|
| `--config <path>` | Config file (prefer before subcommand) |
| `--serve [port]` | Same as `serve` |
| `--host <ADDR>` | With `serve` |
| `--port 0` | With `serve`: OS-assigned free port; startup log and **`web_ready`** **`port`/`url`** use **`local_addr()`** after bind |
| `--desktop-ready-json` | With `serve`: after listen succeeds, print one **`web_ready`** JSON line to **stdout** (for scripts/tools; the desktop shell **no longer** depends on it). **Deprecated name**; prefer alias **`--web-ready-json`** (same behavior) |
| `--web-ready-json` | Alias of `--desktop-ready-json` |
| `--workspace <path>` | Override initial workspace |
| `--no-tools` | Disable tools |
| `--llm-context-tokens <N>` | Override **`[agent] llm_context_tokens`** / **`CM_LLM_CONTEXT_TOKENS`** (`0` = do not override) |
| `--with-web` / `--web` | Explicitly mount business UI static assets (needs `CM_WEB_STATIC_DIR` or probed dist) |
| `--dry-run` | Maps to `config` |
| `--log <FILE>` | Log file + stderr mirror |

> **Removed (D2.1)**: global **`--agent-role`**, and chat/repl-only **`--query` / `--stdin` / `--output` / `--no-stream`**. Use Client **`crabmate-tui`** or Web; roles via request body / Web **`agent_role`**.

## Benchmark (`bench`)

Planning for benchmark features and testing: **`docs/基准测试规划.md`** (kept separate from general CLI product notes).

| Option | Description |
|--------|-------------|
| `--benchmark <TYPE>` | `swe_bench`, `gaia`, `human_eval`, `generic` |
| `--batch <FILE>` | Input JSONL (`human_eval` rows need `humaneval_test` and `entry_point`; see **`docs/基准测试规划.md`** §5) |
| `--batch-output <FILE>` | Default `benchmark_results.jsonl` |
| `--task-timeout <SECS>` | `0` = no limit |
| `--max-tool-rounds <N>` | `0` = **no** round cap (**does not** disable tools); use global **`--no-tools`** to disable |
| `--resume` | Skip existing `instance_id` |
| `--bench-system-prompt <FILE>` | Override system |

HumanEval adapter prepends a “complete the function body, do not ask” instruction and a system suffix. Convert the official JSONL, run `bench`, then score with Python 3 (**executes model-generated code** — sandbox if needed). Prefer global **`--no-tools`** (`--max-tool-rounds 0` is not enough):

```bash
python3 scripts/humaneval_official_to_crabmate_jsonl.py --input HumanEval.jsonl --output tasks.jsonl
cargo run -- --no-tools bench --benchmark human_eval --batch tasks.jsonl --batch-output results.jsonl
python3 scripts/humaneval_score_benchmark_results.py --tasks tasks.jsonl --results results.jsonl
```

## Examples

```bash
cargo run -- serve
cargo run -- --config /path/to/my.toml serve
RUST_LOG=debug cargo run -- --log /tmp/crabmate.log serve
cargo run -- serve 3000
cargo run -- serve --port 3000               # same as above
cargo run -- --workspace /path/to/project serve 8080
cargo run -- serve --host 0.0.0.0            # mind auth & safety
cargo run -- serve --host 127.0.0.1 --port 0 --web-ready-json   # optional: print web_ready for scripts (legacy alias: --desktop-ready-json)
cargo run -- --no-tools serve
cargo run -- bench --benchmark swe_bench --batch tasks.jsonl --batch-output results.jsonl --task-timeout 600
cargo run -- config
cargo run -- doctor
cargo run -- save-session
cargo run -- save-session --format json --workspace /path/to/proj
```

## `save-session`

Reads **`<workspace>/.crabmate/tui_session.json`** by default (`--workspace` and global `--config` before subcommand), writes timestamped **`chat_export_*.json`** / **`chat_export_*.md`** under **`<workspace>/.crabmate/exports/`** (schema / Markdown titles shared via **`crabmate-chat-export`**). JSON envelope (**required** fields): `schema` (`crabmate.chat_session`), `schema_version` (currently **`2.0.0`**), **`projection`** (**`raw`**: full OpenAI-shaped `Message` from CLI/`save-session`/session files; **`display`**: ops display projection — **not** valid **`tool-replay`** input), `version`, `messages`. Markdown / `display` **tool sections** use envelope summaries or truncated raw text and are **not** guaranteed to match Web tool cards character-for-character (pixel tool cards live in Client `crabmate-tool-card`). Legacy JSON missing envelope keys is **rejected**. See `crates/crabmate-chat-export`, `runtime/chat_export.rs`, Client `frontend/src/session_export.rs`. Each stdout line is the absolute path of a written file for scripts.

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

## In-process dialogue entry (removed)

**D2.1**: **`crabmate chat` / `repl` / `tui`** and legacy maps (`--query`, default `repl`, etc.) are **removed from clap**. Official terminal: Client **`crabmate-tui`** (HTTP/SSE → **`serve`**); Web / Desktop still use **`POST /chat`** / **`/chat/stream`**.

Still available in this repo:

- **`save-session`** (alias **`export-session`**): export disk session files to **`.crabmate/exports/`** (same shape as Web).
- **`tool-replay`**: extract/replay tool timelines from session JSON (**no** LLM).
- **`bench`**: batch evaluation.
- Web session persistence, approval (SSE + **`POST /chat/approval`**), export: see HTTP routes and Client UI below.

Exit-code constants remain in **`src/runtime/cli_exit.rs`** / [`CLI_CONTRACT.md`](CLI_CONTRACT.md) (mainly **`tool-replay`** today). In-process **`runtime/cli/{chat,repl}`** and **`runtime/tui`** were removed in **D2.2**; use Client **`crabmate-tui`**.

Hot reload for **`serve`**: **`POST /config/reload`** (see **`docs/en/CONFIGURATION.md`**).

## Frontend build and Web

Business UI lives in [crabmate-client](https://github.com/noisystreet/crabmate-client) (clone as a sibling first):

```bash
cd ../crabmate-client && make frontend
export CM_WEB_STATIC_DIR="$PWD/frontend/dist"
cd ../crabmate_agent && cargo run -- serve --with-web
```

API-only (default): `cargo run -- serve`. Config check: `cargo run -- config` (skips UI dist by default; add **`--with-web`** to require static root).

Static assets are served only with **`--with-web`**, via **`CM_WEB_STATIC_DIR`** (or sibling Client `frontend/dist`).

## Main HTTP routes (`serve`)

### HTTP auth matrix

Matches **`src/web/server.rs`**. When **`web_api_bearer_token` / `CM_WEB_API_BEARER_TOKEN`** is non-empty at startup, protected routes get the Bearer layer (`Authorization: Bearer` **or** `X-API-Key`). Empty secret → no layer (trusted environments only).

| Class | Paths | Bearer layer | Notes |
|-------|-------|--------------|-------|
| Protected API | `/chat*`, `/conversation/*`, `/workspace*`, `/skills`, `/tasks`, `/github/*`, `/config/*`, `/user-data/*`, `/upload`, `GET /uploads/{filename}`, `/uploads/delete` | **Yes** (if token non-empty at start) | See configuration docs |
| Public system | `GET /health`, `GET /status` | **No** | **Not** behind Bearer even when configured; isolate via bind/`127.0.0.1`/firewall/proxy |
| Spec / shell | `GET /openapi.json`, `GET /web-ui` | **No** | |
| Static | `/`, SPA | **No** | |
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
| POST | `/chat/stream/{job_id}/cancel` | Cancel an in-flight SSE turn (`job_id` = **`x-stream-job-id`**). Sets the cooperative cancel flag and cancels that turn’s background **`run_command`** jobs. **200** `{ cancelled, background_tools_cancelled }`; **410** `STREAM_JOB_GONE` if the job is gone (same code as **`stream_resume`**; distinguish by route). **Aborting the SSE connection alone does not cancel** (so `stream_resume` can work). |
| POST | `/chat/approval` | Approval: `approval_session_id`, `decision` |
| POST | `/chat/branch` | Branch/truncate: JSON `conversation_id`, `before_user_ordinal` (0-based plain user index), `expected_revision`; server truncates **before** that user message (same as Web “regenerate from here”: resend the user text via `/chat/stream`). Requires persisted conversation and matching `revision` |
| GET | `/conversation/messages` | Read persisted canonical session: query **`conversation_id`**; optional **`limit`** / **`before_index`**. Response includes **`revision`**, visible **`messages`**, paging fields, optional **`active_agent_role`**, optional **`tiktoken_prompt_tokens`** (legacy pair plus safe-input/component/provider-usage soft fields), optional **`context_artifacts`** (`ModelContextView` replay recipes), and optional session-scoped **`layout`**. Hidden artifacts are not returned as chat rows; canonical history remains complete. **404** `CONVERSATION_NOT_FOUND` |
| POST | `/upload` | Multipart upload (protected); returns file URL list (`UploadResponseBody`). Files live beside the session SQLite (`chat_uploads/` next to `conversations.db`; **not** switched by `POST /workspace`). Session still stores **`/uploads/<filename>`** |
| GET | `/uploads/{filename}` | Chat attachment bytes (same auth as other protected APIs; Client fetches with Bearer into a `blob:` URL). Missing → **404** |
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
| GET | `/workspace/file/raw` | Raw **image bytes** in the workspace (`path` relative; **png/jpg/jpeg/webp/gif** only, no svg; 8 MiB cap; same auth as other protected APIs). Used so chat Markdown `![alt](plots/a.png)` can be fetched with Bearer and shown as a `blob:` URL. Do **not** copy workspace files onto `/uploads` |
| GET | `/workspace/file/download` | Raw **file bytes** of any type including PDF (`path` query; **16 MiB** cap, same as `PUT /workspace/file/raw`; `Content-Type: application/octet-stream`). Used by Client **Save to this device**. Do **not** use `GET /workspace/file/raw` (image allowlist returns **415** `WORKSPACE_IMAGE_UNSUPPORTED`) or `GET /workspace/file` (UTF-8 JSON; binary decode fails) |
| GET | `/workspace/dir/archive` | Zip a workspace **directory** (`path` relative; omit/empty = workspace root; **no** symlink follow; **16 MiB** uncompressed, **256** files, depth **24**). `Content-Type: application/zip`. For saving a folder; single files still use **`GET /workspace/file/download`**. Requires **`archive-tools`** (on by default with `server`) |
| POST | `/workspace/file/move` | Move/rename a regular **file** (JSON `from`, `to`; optional `overwrite`, `conversation_id`). **204** on success; **409** `WORKSPACE_FILE_EXISTS` without `overwrite`. Records the session changelist like **`move_file`** when enabled. Directories are not supported |
| PUT | `/workspace/file/raw` | Write **raw bytes** (`path` query required; optional `create_only` / `update_only`; any type, **16 MiB** cap and the same HTTP body limit on this route, not the ~220 MiB protected-API default; same as JSON `POST /workspace/file`). **204** on success; **409** `WORKSPACE_FILE_EXISTS` when `create_only` and the file exists. Used by Client OS-file drop onto the workspace tree; do **not** use chat `POST /upload` |
| GET | `/workspace/file` | Read file in workspace (`path` required; optional **`encoding`**, same as `read_file`, default UTF-8 strict; 1 MiB cap) |
| POST | `/workspace/file` | Write file (JSON `path`, `content`; optional **`create_only`** / **`update_only`**) |
| DELETE | `/workspace/file` | Delete file (`path` required; not directories) |
| POST | `/workspace/dir` | Create dir (JSON **`path`**, optional **`parents`**); or delete dir (JSON **`delete=true`**, **`confirm=true`**, optional **`recursive=true`**, same as **`DELETE`**; frontend falls back to **`POST`** on 404/405) |
| DELETE | `/workspace/dir` | Delete directory (`path` query required; **`confirm=true`** required; non-empty dirs need **`recursive=true`**) |
| GET | `/health` | Health (workspace writable, optional CLI deps; **not** failed for missing process `API_KEY`; Client supplies `client_llm.api_key`) |

SSE control-plane fields: **`docs/SSE协议.md`**.

## One-shot packaging (tar.gz + optional .deb)

Default is **server-only** (**no** frontend):

```bash
make package           # tar.gz + optional .deb → dist/
make package-tar       # tar.gz only
make package-deb       # .deb only (Linux + cargo-deb)
# equivalent: ./scripts/package-release.sh --skip-frontend
```

Artifacts land in **`dist/`** at the repo root: a **`tar.gz`** is always produced (unless **`--skip-tar`**); **`.deb`** is copied from **`target/debian/`** only on **Linux** with **`cargo-deb`** installed (use **`--skip-deb`** to skip). Run **`./scripts/package-release.sh --help`** for all flags.

## Debian `.deb` package

```bash
cargo install cargo-deb
make package-deb
# or: cargo build --release && cargo deb
sudo dpkg -i dist/crabmate_*.deb   # or target/debian/crabmate_*.deb
```

Server **`make package*`** / `.deb` does **not** embed UI. Runtime is **API-only by default**; host SPA with **`--with-web`** + **`CM_WEB_STATIC_DIR`**. To optionally bundle UI in a tarball, run **`./scripts/package-release.sh --frontend-dist …/frontend/dist`** (Makefile targets do not take that path).

After install: `export API_KEY=… && crabmate serve` (API-only); or **`crabmate serve --with-web`** with **`CM_WEB_STATIC_DIR`**. Package includes **`/usr/share/man/man1/crabmate.1`** (`man crabmate` if **`MANPATH`** includes `/usr/share/man`).

### systemd (`.deb` / tarball)

- **`.deb`**: installs **`/usr/lib/systemd/system/crabmate.service`**, **`/etc/crabmate/config.toml`** (path anchor), **`/etc/crabmate/config/prompts/`**, and **`crabmate.env.example`**; `postinst` creates system user **`crabmate`** and **`/var/lib/crabmate`**. **Does not** `enable` / `start` by default.
- **Defaults**: **`127.0.0.1:8080`**; unit uses **`--config /etc/crabmate/config.toml`**. **API-only by default** (no SPA); add **`--with-web`** and set **`CM_WEB_STATIC_DIR`** to host UI. Without UI, **`/`** is 404 while APIs (e.g. **`/health`**) work.
- **Environment file**: **`KEY=value` only** (no **`export`**); set **`API_KEY`**, and extend **`PATH`** if the system user needs cargo/rustc.
- Before enabling:

```bash
sudo cp /etc/crabmate/crabmate.env.example /etc/crabmate/crabmate.env
sudo chmod 600 /etc/crabmate/crabmate.env   # fill placeholders
sudo systemctl daemon-reload
sudo systemctl enable --now crabmate.service
sudo systemctl status crabmate.service
```

- **tar.gz**: ships **`systemd/`** and **`etc/crabmate/`** (`config.toml`, prompts, env example). Copy **`etc/crabmate`** to **`/etc/crabmate`** and the unit under **`/usr/lib/systemd/system/`** (adjust `ExecStart` if needed). Broader reverse-proxy notes: **[docs/个人VPS部署指南.md](../个人VPS部署指南.md)**.

Preview from tree: `man -l man/crabmate.1` (path relative to repo root).
