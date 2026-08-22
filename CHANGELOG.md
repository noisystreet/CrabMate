# Changelog

All notable changes to **CrabMate** (this server repository) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

中文说明：本文件以英文为主（与默认 [`README.md`](README.md) 一致）；面向使用者的中文入口见 [`README.zh.md`](README.zh.md)。发版时先在 **`[Unreleased]`** 积累条目，打 tag 前再切到版本分区。

## [Unreleased]

### Added

- **`GET /workspace/file/download`**: raw bytes of any workspace file (`path` query, **16 MiB**, `application/octet-stream`). Client **Save to this device** (PDF/binary). `GET /workspace/file/raw` stays image-only.
- **`GET /workspace/dir/archive`**: zip a workspace directory (`path` query or workspace root; **16 MiB** uncompressed, **256** files; no symlink follow).
- **`POST /chat/stream/{job_id}/cancel`**: cooperative cancel for an in-flight SSE turn (`job_id` = `x-stream-job-id`). Also cancels that turn’s background `run_command` jobs. Aborting the SSE connection alone does **not** cancel (so `stream_resume` still works).

### Changed

- (none yet)

### Fixed

- Clicking Stop in the Client previously only aborted the browser fetch; the agent turn and tools kept running. Clients must call the new cancel route.

## [0.5.0] - 2026-08-22

**Git pre-release `v0.5.0-alpha.1`** (follows **`v0.5.0-alpha.0`**). crates.io remains **`0.4.0`** (`cargo install crabmate` does not pick this up). Install from the GitHub Release tarball/`.deb`, or `cargo install --path .` / `--git` at this tag. **Not** a crates.io publish.

SSE wire protocol stays **v2**; background-job fields on AG-UI `TOOL_CALL_RESULT` are soft (old clients ignore them). Chat image embeds need a matching **crabmate-client** build; the model is prompted to write `![alt](relative.png)` rather than copying files onto `CM_WEB_STATIC_DIR`.

### Added

- **`GET /workspace/file/raw`**: serve workspace **png/jpg/jpeg/webp/gif** bytes (relative `path`, 8 MiB cap, same auth as other protected APIs) so Client chat can show `![alt](relative.png)` without copying files onto `/uploads`.
- **`PUT /workspace/file/raw`**: write **raw bytes** into the workspace (`path` query, optional `create_only` / `update_only`; **16 MiB** cap, same as JSON `POST /workspace/file`). HTTP body limit on this route is **16 MiB** (not the ~220 MiB protected-API default). **204** on success. Used by Client OS-file drop (text and binary). `GET` on this path remains image-only. Path segments `.` / `..` are rejected; names like `foo..bar.bin` are allowed.
- System prompt + successful tool-result hint: embed those images with `![alt](relative.png)` in the final reply; do not tell users to copy files onto `CM_WEB_STATIC_DIR`.
- Outbound `chat/completions`: session still stores **`/uploads/<file>`**. Files live next to the session SQLite (**`.crabmate/chat_uploads/`** beside **`conversations.db`**), not the current `POST /workspace` root. Text models drop `image_url` parts; vision models inline JPEG/PNG/GIF/WebP as **`data:`**. Workspace **`@rel.png` / `file:///rel.png`** skip text expansion and are inlined from that turn’s working directory. **`GET /uploads/{filename}`** uses the same auth as other protected APIs; cleanup skips files still referenced in the conversation store.
- **Background tool jobs**: `run_command` can start **`async`** jobs; poll/cancel via **`GET /tools/jobs/{id}`** and **`POST /tools/jobs/{id}/cancel`**. AG-UI v2 forwards job metadata on **`TOOL_CALL_RESULT`**.
- **`http_fetch`**: curl-like **`Accept` / `Accept-Encoding`** (gzip/brotli/deflate), default **`User-Agent` `crabmate/<version>`**, and a hint when the env proxy is **SOCKS** (reqwest 0.13 does not support it).
- Shared subprocess sessions for **`python_snippet_run`**, **`cargo_test`**, and **`pytest_run`** (wall-clock / session stats).
- Host **`run_command`** emits **`tool_output_chunk`** streaming chunks.

### Changed

- **`lizard-rust`**: fail if any `src/` function has CCN > 10; drop per-module count/sum ratchets and `scripts/lizard_module_ccn_caps.toml`. Split remaining high-CCN helpers in `src/cm_*` so the global cap holds.
- On timeout or cancel, **`run_command`** kills the **process group**, not only the direct child.
- DeepSeek vendor match also uses **`model_id_prefixes = ["deepseek-"]`**, so a proxy `api_base` without the substring `deepseek` still gets text-vs-vision image handling.

### Fixed

- Treat reqwest **`SendRequest`**-class transport errors as retryable (same backoff as other transient LLM HTTP failures).
- Upgrade **`h2`** for **RUSTSEC-2026-0258**.
- LLM HTTP retries clone the original `ChatRequest` each attempt so debug request previews do not dump inlined base64.

## [0.4.0] - 2026-08-16

First **crates.io** release of the single crate **`crabmate`** (default feature **`server`**; Client pins **`protocol`**). Install: **`cargo install crabmate`**. Git tag **`v0.4.0`** matches this package; do **not** treat **`v0.3.0`** as crates.io `0.3.0`.

### Changed

- **Public API docs**: rustdoc and README state the `0.4.0` semver whitelist (`protocol` six modules; `server` composition names + explicit `pub use`). `#[doc(hidden)]` is not a stable SDK.
- **`GET /openapi.json`**: document `/user-data/mcp-servers*` and `PUT /user-data/secrets/web-api-bearer`; test that OpenAPI **path+method** pairs match axum `.route(` in source (not a hand-maintained list; not E2E fixtures / static files).
- **Package metadata**: `repository` / `readme` / `include` for the crates.io tarball; maintainer docs point at `src/cm_*` instead of old workspace package names.
- **Client contract `client-contract-v0.2.0`**: drop `crabmate-tool-card` from this repo (Client vendors it). `GET /conversation/messages` no longer fills `display_*` on `role=tool`; `save-session` Markdown tool sections are no longer pixel-aligned with Web cards. Remaining pin crates are unchanged at Cargo `0.1.0`.

## [0.3.0] - 2026-08-15

Server cut after **D2.2**: in-process `chat` / `repl` / `tui` are gone; official terminal is Client **`crabmate-tui`**. Also drops the Feishu IM sidecar crate from this workspace.

### Added

- **`bench --samples N`**: multi-sample HumanEval scoring with unbiased **pass@k** aggregation (`humaneval_score_benchmark_results.py --k`); records pass@1 baselines (first 30 and full 164).
- **`gh_pr_create`**: annotates common GraphQL failures with actionable hints.

### Changed

- **BREAKING (D2.2)**: Hard-delete in-process **`chat` / `repl` / `tui`** implementation and Cargo features **`repl`/`tui`** (official terminal: Client **`crabmate-tui`**). Default features are **`web` + `mcp`**.
- **BREAKING**: Removed in-process CLI tool runtime and terminal approval. Interactive tool approval is **Web SSE** only.
- **BREAKING**: **`web_chat_json`** (`POST /chat`) no longer echoes assistant/tool transcript to the **`serve`** process stdout; remove direct dependencies on the in-process terminal render stack (`termimad` / `crossterm` / `indicatif`); **`CM_CLI_WAIT_SPINNER`** is ignored. **`unicode-width`** may remain transitively (e.g. via `console` for `web-bearer`).
- **BREAKING**: Removed **`--no-web`** / **`--cli-only`** (they were no-ops after API-only default). Use bare **`serve`** / **`config`** for API-only; **`--with-web`** to mount or check UI dist. Scripts that still pass those flags will get clap unknown-argument errors.
- **BREAKING**: TOML keys **`tui_load_session_on_start`**, **`tui_session_max_messages`**, **`repl_initial_workspace_messages_enabled`** rejected under `[agent]` (**`deny_unknown_fields`**). Remove them from user `config.toml`. Legacy **`CM_TUI_*` / `CM_REPL_*`** env vars for those settings are ignored. **`GET /status`** no longer reports the two session-UI booleans. Historical path **`.crabmate/tui_session.json`** remains for **`save-session`** / **`tool-replay`**.
- **BREAKING**: Removed **`CM_GITHUB_OAUTH_CLIENT_ID`** and **`/user-data/secrets/github*`**. Device Flow carries `client_id` in the request body; git/gh credentials are request-scoped (header/Cookie). Requires a matching Client upgrade.
- **BREAKING**: Server no longer reads/writes **`client_llm` / `executor_llm` / `saved_model_*`** keyring slots; **`PUT` client-llm** is removed. Secrets status for those slots is always unset. **`web_api_bearer`** and MCP bearer remain. LLM keys come from the Client request (`client_llm.api_key`) or process **`API_KEY`**.
- **BREAKING**: Removed workspace crate **`crabmate-im-bridge`**. Feishu/IM sidecar is no longer part of this server repository.
- **`GET /health`** and `serve` startup no longer treat a missing process **`API_KEY`** as a required failure (Client supplies `client_llm.api_key`).
- **`run_command` tool-call summary**: join `command` + `args` even when `args` is a JSON string (not only a string array); quote tokens that contain spaces. Tool cards also read `run_command_exit_v1.invocation` when the output body no longer starts with `命令：`.
- **`run_command`**: when argv needs glob / `$VAR` / `~` and **`bash`/`sh`** are allowlisted, join into one script and run **`bash -c`**. Standalone `&&` / `|` / `;` also wrap, but **Web always re-approves** that script (so `ls && rm` cannot silently bypass the single-command allowlist). `?` / `[` are not glob unless the token looks like a path glob (`file?.c`). Wrapped **`gh`** still receives **`GH_TOKEN`**. Plain argv still uses `execve`. Without bash/sh, expansion requires Web approval of the full script.
- **`bench --benchmark human_eval`**: adapter prepends a function-completion instruction and a system suffix so the default coding workbench does not ask clarifying questions; **`extract_humaneval_completion`** strips a repeated function prefix before scoring. Prefer global **`--no-tools`** (`--max-tool-rounds 0` does not disable tools).
- Produce **`crabmate_tool_output`** headers from serde structs (same on-wire contract).
- Maintainer docs point Client links at GitHub rather than a sibling checkout path.

### Fixed

- Judge tool success/failure from the **`crabmate_tool_output`** header contract, not from failure keywords inside the payload (avoids false “failed” when diffs or logs mention 失败).
- **`read_file`**: conflict errors show mutually exclusive `anchor_line` vs range usage.
- **`gh_pr_checks`**: fall back to table output when `gh pr checks --json` is unsupported.
- Enable **`console`** `std` feature so `Term` compiles after replacing `dialoguer`.

## [0.2.0] - 2026-08-09

Server cut after path-A Client split follow-ups: API-only `serve` by default, `--with-web`, and default CORS for official shells.

### Added

- Dev/package **Dockerfile** (Ubuntu **24.04** toolchain + `cargo-deb`) and **`make package-docker`** to produce host `dist/*.tar.gz` / `dist/*.deb` (not a runtime image; UI/Trunk stays in Client).

### Changed

- Default **`web_cors_allowed_origins`**: official Client shell Origins **`tauri://localhost`** + **`http://tauri.localhost`** (CORS on without env). Explicit empty env/TOML list still disables CORS; non-empty lists are merged with those defaults.
- **BREAKING**: `serve` is **API-only by default** (no SPA mount). Host Client UI with **`--with-web`** / **`--web`** plus **`CM_WEB_STATIC_DIR`** (or probed dist). **`--no-web`** / **`--cli-only`** remain as compat no-ops (mutually exclusive with **`--with-web`**). **`config` / dry-run** now **skips** UI static checks by default (previously required dist unless `--no-web`); add **`--with-web`** to require dist. Docs, systemd comments, and man page updated accordingly.
- Default pure-API `serve` skips `resolve_web_static_dir` / SPA probe; static root is resolved only with **`--with-web`**.
- Deb `depends` → **`libc6 (>= 2.39)`** to match binaries built on Ubuntu 24.04 / current CI (`ubuntu-latest`).
- **`lizard-rust`** gate: per-module **count of functions with CCN>10** (exact ratchet vs `scripts/lizard_module_ccn_caps.toml`), aligned with Client; replaces per-module max-CCN caps (`global_ccn_ceiling` / `ccn_max`).
- **`lizard-rust`**: also ratchets **`global_over_ccn_sum_cap`** — the sum of CCN across all functions with CCN>threshold (full-repo scans).
- Refactor small modules that had 1–2 functions with CCN>10 (meta dialogue, chat job queue, MCP, CLI serve, turn replay dump, e2e dump/judge, terminal render, runtime display/LaTeX, turn-layout, `cmd_mate`) so those module caps are **0**.
- Refactor `src/agent` outer-loop / reflect / serial early-emit / context summary helpers so its CCN>10 count is **0**.
- Refactor `src/runtime` (REPL/TUI/CLI helpers) so its CCN>10 count is **0**; full-repo over-threshold CCN sum ratchet lowered accordingly.

### Fixed

- (none)

## [0.1.0] - 2026-08-08

First public **server** release tag (`v0.1.0`). Cargo package version was already `0.1.0`; this changelog marks the cut for GitHub Release / installable artifacts.

**Scope**: this repo is the Agent **server** (HTTP API, tools, SSE). At this tag the tree still included in-process **CLI/REPL/TUI**; those entries were **removed in D2.2** (see [0.3.0]). Official Web UI and desktop/Android shells live in [`crabmate-client`](https://github.com/noisystreet/crabmate-client) (path A, Phase 4.2 complete).

### Added

- OpenAI-compatible `chat/completions` client (DeepSeek, MiniMax, Zhipu GLM, Moonshot Kimi, Ollama, …) with streaming, retries, and tool calling.
- HTTP **`serve`**: `/chat`, `/chat/stream` (SSE / AG-UI), workspace APIs, conversation SQLite under `.crabmate/`, optional Web API Bearer.
- Built-in tool registry (`run_command` allowlist, file tools, fetch, cargo/npm stacks, workflows, optional MCP / Docker sandbox / fastembed via Cargo features).
- CLI: `serve`, `doctor`, `models` / `probe`, `save-session`, `mcp`, packaging helpers. Historical at this tag also: `repl`, `tui`, `chat` (removed in D2.2).
- CLI **`web-bearer status|set|clear`**: persist the Web API shared secret in the system keyring (same slot as Web Settings); **`serve`** falls back when TOML / **`CM_WEB_API_BEARER_TOKEN`** are empty. Prefer **`set --stdin`** / **`set --from-env`** / interactive hidden input to avoid putting the secret on argv.
- Client contract versioning gates (`client-contract-v*`) and CI smoke for SSE / OpenAPI / consumer pins.
- Release packaging: `make package` → server-only **tar.gz** + **`.deb`**; **systemd** unit (`crabmate.service`), `/etc/crabmate/config.toml` + `config/prompts/`, env example (`KEY=value` only).
- GitHub Actions **Release** workflow (`.github/workflows/release.yml`): tag `vX.Y.Z` (or `vX.Y.Z-rc.N`) → `make package` + GitHub Release with tar.gz/deb; notes from this file’s core `X.Y.Z` section; re-run updates the same Release.
- Default English [`README.md`](README.md) with Chinese companion [`README.zh.md`](README.zh.md).

### Changed

- Path A: removed in-repo `frontend/`, desktop/mobile shells, and Playwright ownership from this repo; document pointers target Client.
- CI package job: server-only artifacts; test job avoids full `cargo clean` (clears incremental only when free disk is low).
- Packaged unit uses `--config /etc/crabmate/config.toml` (prompt path anchor); does **not** force `--no-web` so `CM_WEB_STATIC_DIR` can mount a Client-built UI.

### Fixed

- `crabmate-gen-man` packaging requires `--features gen-man`.
- Deb package smoke grep paths aligned with `dpkg-deb -c` output (`./usr/bin/…`).

### Known limitations (0.1.0)

- **Trusted workspace** model: `run_command` allowlist includes powerful tools (`bash`, `git`, `cargo`, …). Not a multi-tenant SaaS.
- Chat job queue is **single-process**; no Redis/SQS horizontal scale yet.
- Process auth is shared **Bearer** (optional); no per-user accounts in-process (use a gateway/BFF if needed).
- Default Cargo features include **`mcp`** (with `web`); in-process **`repl`/`tui` features removed** (D2.2). Heavy options such as **`fastembed`** remain opt-in.
- Systemd service user has a **minimal `PATH`**; extend via `/etc/crabmate/crabmate.env` for host toolchains. Bypass HTTP proxies for `127.0.0.1` when probing locally.
- Compatibility-layer shrink items **B2–B4**, full unwrap audits, and agent benchmarks remain backlog ([`docs/待办清单.md`](docs/待办清单.md)).

[Unreleased]: https://github.com/noisystreet/CrabMate/compare/v0.5.0-alpha.1...HEAD
[0.5.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.5.0-alpha.1
[0.4.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.4.0
[0.3.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.3.0
[0.2.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.2.0
[0.1.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.1.0
