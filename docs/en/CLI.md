**Languages / 语言:** [中文](../CLI.md) · English (this page)

# CLI and subcommands

Help: `crabmate --help`, `crabmate help`, `crabmate help <subcommand>` (same as `--help`). Root and **`chat --help`** footers cross-reference **`docs/CLI_CONTRACT.md`** and **`docs/SSE_PROTOCOL.md`**. **Global options** go **before** the subcommand: `--config`, `--workspace`, `--agent-role`, `--no-tools`, `--log`.

**Script contract** (exit codes, `chat --output json` line JSON `type`/`v`, etc.): [`CLI_CONTRACT.md`](CLI_CONTRACT.md).

## Man page (troff / `man`)

- **Source tree**: Pre-generated **`man/crabmate.1`** (troff), aligned with current `clap`; **Debian `.deb`** installs to **`/usr/share/man/man1/crabmate.1`** (see root `Cargo.toml` `[package.metadata.deb] assets`).
- **Regenerate** (after adding/removing subcommands or global flags): `cargo run --bin crabmate-gen-man`, then commit the updated `man/crabmate.1`.
- **`cargo install`**: Does **not** install man into `MANPATH` by default; copy `man/crabmate.1` to `.../share/man/man1/` and run `mandb` (distro-dependent), or prefer **`cargo deb`** / distro packages.

## Subcommand overview

| Subcommand | Description |
|------------|-------------|
| `serve [PORT]` | Web UI + HTTP API, default **8080**; `serve --host <ADDR>` bind address (default `127.0.0.1`). `--no-web` / `--cli-only` API only. |
| `repl` | Interactive chat; **default when no subcommand**. |
| `chat` | One-shot / scripted chat: `--query` / `--stdin` / `--user-prompt-file`, `--system-prompt-file`, `--messages-json-file`, `--message-file` (JSONL), `--yes` / `--approve-commands`, `--output json`, `--no-stream`. |
| `bench` | Batch eval: `--benchmark`, `--batch`, etc. |
| `config` | Config + `API_KEY` self-check; optional `--dry-run`. |
| `doctor` | Local diagnostics (**no** `API_KEY`). |
| `models` | `GET …/models` (needs `API_KEY`). |
| `probe` | Probe models endpoint (needs `API_KEY`). |
| `save-session` | Export JSON/Markdown from session file to workspace **`.crabmate/exports/`** (same shape as Web; **no** `API_KEY`). `--format json|markdown|both` (default `both`), optional `--session-file`. Alias **`export-session`**. |
| `tool-replay` | Extract **tool-call timeline** from session JSON as fixture, or **replay tools** from fixture via `run_tool` (**no** LLM; **no** `API_KEY`). See “Tool replay fixture” below. |
| `mcp list` | Read-only list of in-process MCP stdio sessions matching current `mcp_enabled` + `mcp_command` fingerprint and merged OpenAI tool names (**no** `API_KEY`). If no chat has run yet, **`mcp list --probe`** tries one connection (starts configured MCP child, same as normal chat). |

## Log levels

Without `RUST_LOG`: `serve` defaults to **info**; `repl` / `chat` / `bench` / `config` / `mcp` / `save-session` (and alias `export-session`) / `tool-replay` default to **warn**. Use `RUST_LOG` or `--log <FILE>`.

## Message pipeline debug logs

With `RUST_LOG=crabmate=debug`, each model call prints **`message_pipeline session_sync`** summary; finer: `RUST_LOG=crabmate::message_pipeline=trace`. See **`docs/DEVELOPMENT.md`** § Architecture → **Context pipeline (observability)** and `GET /status` counters; implementation in `src/agent/message_pipeline.rs`.

## Legacy usage

Without a subcommand, legacy flags `--serve`, `--query`, `--benchmark`, `--dry-run`, etc. still map internally. If argv **anywhere** contains an explicit subcommand name (`serve`, `doctor`, `save-session`, `export-session`, `tool-replay`, …), the default `repl` is **not** inserted (see `tests/fixtures/cli/legacy_normalize.json`).

## Common options (compat)

| Option | Description |
|--------|-------------|
| `--config <path>` | Config file (prefer before subcommand) |
| `--serve [port]` | Same as `serve` |
| `--host <ADDR>` | With `serve` |
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

| Option | Description |
|--------|-------------|
| `--benchmark <TYPE>` | `swe_bench`, `gaia`, `human_eval`, `generic` |
| `--batch <FILE>` | Input JSONL |
| `--batch-output <FILE>` | Default `benchmark_results.jsonl` |
| `--task-timeout <SECS>` | `0` = no limit |
| `--max-tool-rounds <N>` | `0` = no limit |
| `--resume` | Skip existing `instance_id` |
| `--bench-system-prompt <FILE>` | Override system |

## Examples

```bash
cargo run                                    # default repl
cargo run -- --config /path/to/my.toml serve
RUST_LOG=debug cargo run -- --log /tmp/crabmate.log repl
cargo run -- serve
cargo run -- serve 3000
cargo run -- --workspace /path/to/project serve 8080
cargo run -- serve --host 0.0.0.0            # mind auth & safety
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

Reads **`<workspace>/.crabmate/tui_session.json`** by default (`--workspace` and global `--config` before subcommand), writes timestamped **`chat_export_*.json`** / **`chat_export_*.md`** under **`<workspace>/.crabmate/exports/`** (same contract as Web; see `runtime/chat_export.rs` and `frontend/src/chatExport.ts`). Each stdout line is the absolute path of a written file for scripts.

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

**Startup banner**: Interactive CLI prints sections—**model** (truncated `api_base`, `llm_http_auth`, `temperature`, `llm_seed`, current **`--no-stream`**), **workspace & tools**, **slash commands**, **key config** (`max_tokens`, `max_message_history`, API timeouts/retries, `run_command` timeout/output caps, staged planning, optional session restore/MCP/long-term memory, etc.). Styling matches **`cli_repl_ui`** `/help`; **`NO_COLOR`** or non-TTY disables ANSI. **`/config`** reprints a **key config summary** anytime (same family as banner, **no** secrets).

**Optional**: **`AGENT_CLI_WAIT_SPINNER=1`** shows stderr spinner and elapsed time while waiting for the **first** streaming chunk (or full body with **`--no-stream`**); default off; needs stderr TTY and no **`NO_COLOR`**. See **`docs/CONFIGURATION.md`**.

**Staged planning (terminal)**: To hide **no-tools planner** model text in interactive CLI, set **`staged_plan_cli_show_planner_stream = false`** or **`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM=0`** (step queue summary and execution steps still apply; see **`docs/CONFIGURATION.md`**). By default there is an extra no-tools **optimizer** round after first **`agent_reply_plan`**; disable with **`staged_plan_optimizer_round = false`** or **`AGENT_STAGED_PLAN_OPTIMIZER_ROUND=0`**.

**SyncDefault Docker (CLI + `chat`)**: Optionally run **SyncDefault** and some tools inside **Docker** after host approval/allowlist (**`sync_default_tool_sandbox_mode = docker`**, image, `user`, etc.; Unix often uses **effective uid:gid** for workspace ownership). Full notes in **`docs/CONFIGURATION.md`** § SyncDefault Docker sandbox.

**Feedback style**: Success/error lines start with **✓** / **✗**; with **`NO_COLOR`** or non-TTY use **`[ok]` / `[err]`** (ASCII).

Slash commands: **`/help`**, **`/clear`**, **`/model`**, **`/config`** (no args), **`/doctor`** (same as **`crabmate doctor`**), **`/probe`** (same as **`crabmate probe`**), **`/models`** / **`/models list`** (same as **`crabmate models`**), **`/models choose <id>`** (set in-memory **`model`** from latest **`GET …/models`** list, unique case-insensitive prefix; persist via config; **`/config reload`** overwrites from disk), **`/agent`** / **`/agent list`** (list configured role ids, same source as **`GET /status`** **`agent_role_ids`**; prints a hint when multi-role is not configured), **`/agent set <id>`** (set this REPL process’s **`agent_role`** (id must exist in the role table) and **rebuild bootstrap messages** with the new system prompt, clearing the rest of the transcript), **`/workspace`** / **`/cd`**, **`/tools`**, **`/export`** (optional `json` / `markdown` / `both`, default `both`; **current memory**), **`/save-session`** (same format args; reads disk **`tui_session.json`**, same as **`crabmate save-session`**). `quit` / `exit` / Ctrl+D exit.

**Tab completion** (interactive TTY, **reedline**): Under the “me:” prompt, if the line before the cursor (trimmed) starts with **`/`**, **Tab** opens slash-command completion (arrows or Tab to select; single match may auto-fill). After **`/export`** or **`/save-session`**, **Tab** completes **`json` / `markdown` / `md` / `both`**. After **`/mcp`**: **`list`**, **`probe`**, **`list probe`**. After **`/models`**: **`list`**, **`choose`** (**`choose`** gets a trailing space for model id). After **`/agent`**: **`list`**, **`set`** (**`set`** gets a trailing space for role id). Completion is off in **`bash#:`** local shell mode.

**`/mcp`**: Read-only MCP stdio cache and merged tool names (same as **`crabmate mcp list`**); **`/mcp probe`** or **`/mcp list probe`** tries one connection (starts **`mcp_command`**). **`/version`**: `crabmate` version and **`OS`/`ARCH`** (no secrets).

**`/config reload`**: Re-merge TOML from **`config.toml`** / **`.agent_demo.toml`** (or **`--config`**) with current env into memory **`AgentConfig`**—**`api_base`**, model, timeouts, allowlists, MCP, **re-read `system_prompt_file`**, etc.; **does not** reopen session SQLite or rebuild shared **`reqwest::Client`**; **`API_KEY`** still env-only. Web equivalent: **`POST /config/reload`**. If Bearer middleware was enabled at startup, toggling token still needs **`serve` restart**. See **`docs/CONFIGURATION.md`** § Hot reload.

**Tool stdout**: After each tool in interactive CLI / **`chat`** (no SSE), prints **`### Tool · …`** title and body. **`read_file`**, **`read_dir`**, and **`list_tree`** print a **terminal summary** (headers + first N lines of content; lines may be truncated) and note that the full output is in history; other tools print the body (truncated if over limit). Full tool results stay in history for the model. On **failure** (non-zero `run_command`, `错误：` / error-prefix style messages, etc.), terminal may print a **self-heal hint · diagnostic command bundle**: one JSON line for the model to call **`playbook_run_commands`** (same heuristics as **`error_output_playbook`**, but **executes** allowlisted `run_command`; **sanitize** `error_text` first). Commands are **not** auto-run.

### Leading `$` (local shell boundary)

On **interactive TTY**, when the input buffer is **empty**, **`$`** (or fullwidth **`＄`**) **without Enter** toggles between “me:” and **`bash#:`**; still supports a line that is only **`$`** then Enter. In **`bash#:`**, one line runs as **local shell** via **`sh -c`** (Windows **`cmd /C`**) in the current workspace directory—**not** the model, **not** `run_command` allowlist—same as typing in your own terminal (any `sh -c` program; stdin cleared). If the line already has text, **`$` inserts normally** (e.g. dollar amounts). **Trusted machine / workspace only**; for controlled commands, use the model with `run_command`. Pipes/non-TTY: inline **`$ <cmd>`**. TTY history: **`.crabmate/repl_history.txt`** in the workspace (separate from model session file).

On model/network failure, interactive CLI prints error and **continues**; use **`/clear`** if history is inconsistent (keeps current `system`).

## `run_command` terminal approval

If the command is not allowlisted: when **stdin** and **stderr** are TTY, **stderr** shows a **dialoguer** menu (arrows; **`NO_COLOR`** plain theme); otherwise **non-interactive**: print instructions, read one line—**y** once; **a** / **always** allow this command name for the session; **n** / Enter deny (good for `echo y` in CI). **`chat --yes`** auto-approves non-whitelist **`run_command`** and unmatched-prefix **`http_fetch` / `http_request`** (very dangerous). **`chat --approve-commands a,b`** adds extra allowed **command names** only (not HTTP URLs).

## CLI vs Web (persistence / approval / export)

| Capability | Web (`serve`) | CLI |
|------------|---------------|-----|
| **Session persistence** | Optional SQLite (`conversation_store_sqlite_path`) + `conversation_id`, multi-session, survives restart (TTL/limits; see `docs/DEVELOPMENT.md`). | **Partial**: interactive CLI optional load/save **`.crabmate/tui_session.json`** (`tui_load_session_on_start` / `tui_session_max_messages`), **single** chain file, **not** Web’s per-`conversation_id` DB. `chat` does not persist across invocations by default; use `--messages-json-file`, etc. **`repl_initial_workspace_messages_enabled`** (default false; see `docs/CONFIGURATION.md`): when true, CLI builds **`initial_workspace_messages`** in background (profile, deps, disk restore); when false, startup is one `system` only—no tokei / `cargo metadata` on boot. |
| **Human approval** | Non-whitelist `run_command`, **`http_fetch` / `http_request`** without `http_fetch_allowed_prefixes` match: SSE control plane + **`POST /chat/approval`** (non-stream `/chat` without approval session may reject). | **`run_command`**: see above (TTY menu / pipe). **`http_fetch` / `http_request`**: same approval; permanent key for **`http_request:<METHOD>:<URL>`** vs **`http_fetch:`**. |
| **Export chat** | Frontend export JSON/Markdown (shape aligned with `.crabmate/tui_session.json`; see `README.md`). | **`save-session`** (alias **`export-session`**) from disk session → **`.crabmate/exports/`**; interactive **`/save-session`** same; **`/export`** exports **in-memory** messages. `chat --output json` is **not** full session export. |

Keep this section in sync with `README.md` when export behavior changes.

## Frontend build and Web

```bash
cd frontend && npm install && npm run build && cd ..
cargo run -- serve
```

Static assets are served from `frontend/dist`.

## Main HTTP routes (`serve`)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Frontend |
| POST | `/config/reload` | Hot-reload in-memory `AgentConfig` (not SQLite path); body `{}` ok; see **`docs/CONFIGURATION.md`** |
| POST | `/chat` | JSON chat; optional `conversation_id`, `agent_role` (new server-side session only), `temperature`, `seed`, `seed_policy` |
| POST | `/chat/stream` | SSE; optional `approval_session_id`, `agent_role` (same); header `x-conversation-id` |
| POST | `/chat/approval` | Approval: `approval_session_id`, `decision` |
| POST | `/chat/branch` | Branch/truncate session (see dev doc) |
| GET | `/status` | Backend status |
| GET | `/workspace` | Workspace list |
| GET | `/workspace/profile` | Project profile Markdown |
| GET | `/workspace/file` | Read file in workspace (`path` required; optional **`encoding`**, same as `read_file`, default UTF-8 strict; 1 MiB cap) |
| GET | `/health` | Health |

SSE control-plane fields: **`docs/SSE_PROTOCOL.md`**.

## Debian `.deb` package

```bash
cargo install cargo-deb
cd frontend && npm install && npm run build && cd ..
cargo build --release
cargo deb
sudo dpkg -i target/debian/crabmate_*.deb
```

After install: `export API_KEY=… && crabmate serve 8080`. Package includes **`/usr/share/man/man1/crabmate.1`** (`man crabmate` if **`MANPATH`** includes `/usr/share/man`).

Preview from tree: `man -l man/crabmate.1` (path relative to repo root).
