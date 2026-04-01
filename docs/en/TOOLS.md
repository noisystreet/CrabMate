**Languages / 语言:** [中文](../TOOLS.md) · English (this page)

# CrabMate built-in tools reference

This document describes built-in tools, common function-calling JSON examples, and release-check troubleshooting. Workspace browsing, streaming, and session export remain in the root [`README.md`](../../README.md) “Features”; day-to-day use also starts there.

**Optional “explain card”** (`tool_call_explain_enabled`): When enabled, every **non-read-only** built-in tool (same notion as `tool_registry::is_readonly_tool(&AgentConfig, name)`, including `run_command`, `run_executable`, file writers, `http_request`, mutating git, `workflow_execute`, …; overridable via **`[tool_registry] write_effect_tools`**) must include top-level string **`crabmate_explain_why`** describing intent in natural language; the server validates length and **strips** it before execution to avoid conflicting with `additionalProperties: false`. Read-only tools and MCP proxies do not require it; MCP still strips the key before forwarding. Complements command/HTTP **approval** (approval authorizes, explain card improves traceability).

**Long outputs in model context** (`tool_result_envelope_v1`, default on): History `role: tool` uses the **`crabmate_tool`** JSON envelope (`summary`, `output`, …) plus **`tool_call_id`**, **`execution_mode`** (`serial` / `parallel_readonly_batch`), **`parallel_batch_id`** (shared within a parallel read-only batch); failures may include **`retryable`** (heuristic with `error_code`, not a guarantee). **Parallel read-only batches** use the same wall-clock as serial **`dispatch_tool`** (`tool_registry::parallel_tool_wall_timeout_secs`; e.g. **`http_fetch`** uses **`max(http_fetch_timeout_secs, command_timeout_secs)`**, **`get_weather` / `web_search`** use their own timeouts, others often **`command_timeout_secs`**); on timeout the text mentions timeout and **`tool_result.error_code`** is **`timeout`**. Before each model request, if over **`tool_message_max_chars`**, the server head/tail-samples **`output`** and sets **`output_truncated`**, **`output_original_chars`**, **`output_kept_head_chars`**, **`output_kept_tail_chars`** so one grep/build log cannot fill the window; full text may still appear via SSE/export depending on UI. SSE **`tool_result`** carries the same correlation fields. See **`docs/en/DEVELOPMENT.md`**, **`docs/en/SSE_PROTOCOL.md`**, **`docs/en/CONFIGURATION.md`**.

**Session workspace changelist** (default on, `session_workspace_changelist_*`): Successful writes from **`create_file` / `modify_file` / `copy_file` / `move_file` / `delete_file` / `append_file` / `search_replace` / `apply_patch` / `structured_patch`** accumulate **relative paths** and a unified diff vs first touch this session; before **each** model request a **`user`** is injected (**`name=crabmate_workspace_changelist`**). Web strips this before saving sessions (like long-term memory injection). Tools inside **`workflow_execute`** DAG use a separate `ToolContext` and **do not** hit this table.

**Final answer `agent_reply_plan` v1** (validated on workflow reflection paths): `steps[].id` must be unique and match stable character rules (see **`docs/en/DEVELOPMENT.md`**); optional **`workflow_node_id`** aligns with **`nodes[].id`** from the latest **`workflow_execute`** tool result (subset check).

**Optional Docker sandbox** (`sync_default_tool_sandbox_mode = docker`): **SyncDefault** and **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** run in a **one-shot Docker container** (bollard → Engine API) after host approval/allowlist; **`workflow_execute`** and **MCP** stay on the host. Requires reachable **Docker daemon** and non-empty **`sync_default_tool_sandbox_docker_image`** with the CLIs you need (`git`, `rg`, `cargo`, …) and **same CPU arch** as the host `crabmate` binary (mounted read-only; repo ships no fixed image). Default **no network** (`docker_network` empty); for weather/search/HTTP egress set **`sync_default_tool_sandbox_docker_network`** (e.g. `bridge`). Steps, sample Dockerfile, env, and safety notes: [`docs/en/CONFIGURATION.md`](CONFIGURATION.md) § SyncDefault Docker sandbox.

## Built-in tools (model-callable)

- **Many built-in tools; the model picks as needed**:
  - `get_current_time`: Current date/time.
  - `calc`: Math via Linux `bc -l` (arithmetic, `^`, sqrt/sin/cos/tan/ln/exp, pi/e, …).
  - `convert_units`: Physical/data **unit conversion** ([`uom`](https://crates.io/crates/uom), no external process). `category`: length / mass / temperature / data / time / area / pressure / speed (or Chinese aliases); `value` + `from` + `to`; decimal KB/MB/GB vs binary KiB/MiB/GiB for data.
  - `get_weather`: Weather by city/region ([Open-Meteo](https://open-meteo.com/), no key).
  - `web_search`: **Web search** ([Brave](https://brave.com/search/api/) or [Tavily](https://tavily.com/)); set `web_search_api_key` and `web_search_provider` (`brave` / `tavily`). Without a key the tool returns an explanatory error. Prefer `search_in_files` for exact string/regex; use `codebase_semantic_search` for semantic matches (needs `rebuild_index`; see below and **`docs/en/CONFIGURATION.md`**).
  - `http_fetch`: **GET** (default) or **HEAD**. GET returns status, Content-Type, **redirect chain**, body (timeouts/size caps); **HEAD** skips body. URLs matching `http_fetch_allowed_prefixes` (**same origin + path prefix boundary**) run immediately; otherwise Web (`/chat/stream` + `approval_session_id`) or **CLI** can approve **deny / once / always** (GET/HEAD share normalized whitelist key; CLI: `tool_approval::cli_terminal`).
  - `http_request`: **POST / PUT / PATCH / DELETE** (optional `json_body`). Same prefix rules; unmatched URLs use the same approval path (**permanent key** `http_request:<METHOD>:<URL>` vs `http_fetch:`). **`workflow_execute` nodes** still require whitelist match (no approval on sync path). Returns status, Content-Type, redirects, body preview (dry-run first; never put real secrets in bodies).
  - `run_command`: Whitelisted read/query Linux commands (`ls`, `pwd`, `whoami`, `date`, `cat`, `file`, `head`, `tail`, `wc`, `cmake`, `ctest`, `mkdir`, `ninja`, `gcc`, `g++`, `clang`, `clang++`, `c++filt`, `autoreconf`, `autoconf`, `automake`, `aclocal`, `make`, GNU Binutils read-only tools `objdump`, `nm`, `readelf`, `strings`, `size`, default also `ar`, …) with timeout and output truncation. **GitHub CLI**: allowlist includes **`gh`** (install locally and authenticate, e.g. `gh auth login`); same arg rules as `git`—no `..` and no `/`-prefixed args—use `owner/repo` or relative paths, not absolute filesystem paths in arguments. Missing CLI → **`dep_gh`** degraded on **`GET /health`**; **`crabmate doctor`** reports `gh` availability in the toolchain section. **CMake/ctest** are allowlisted; args must not contain `..` or start with `/`; prefer relative build dirs (avoid absolute `-D`). Missing tools may show `dep_cmake` / `dep_ctest` degraded on `/health`. **mkdir**: creates dirs (complements **`create_dir`**). **c++filt**: demangle C++ symbols. **Binutils**: missing → corresponding `dep_*` degraded. **Autotools**: trusted workspaces only. **Test output cache** (`test_result_cache_enabled`): when **`command` is `cargo` and `args` start with `test`** and **omit** `--nocapture` / `--test-threads`, shares in-process LRU with `cargo_test`; hits prefix output with **`[CrabMate test output cache hit]`**.
  - `run_executable`: Run a **relative-path** executable under the workspace (e.g. `./main`, build artifacts). Use this for workspace binaries—not `run_command`.
  - `package_query`: Read-only Linux package info (apt/rpm abstraction): installed?, version, source. `manager=auto|apt|rpm` (default `auto`, tries `dpkg-query` then `rpm`); no install/remove.
  - `delete_file` (needs `confirm`) / `delete_dir` (needs `confirm`, optional `recursive`).
  - `append_file`: Append to file (`create_if_missing` optional).
  - `create_dir`: Create directory (default `parents=true`, `mkdir -p` style).
  - `search_replace`: Single-file replace (literal or regex; default `dry_run`; write needs `confirm`).
  - `create_file` / `modify_file`; `read_file` supports ranges, line caps, **`encoding`** (`utf-8` strict, `utf-8-sig`, `gb18030`/`gbk`/`big5`, `utf-16le`/`be`, `auto`, …; malformed → error). `modify_file` can replace line ranges. **Per-turn** `read_file` cache (mtime+size; cleared on writes / workspace change), key includes **encoding**, cap **`read_file_turn_cache_max_entries`**. Web `GET /workspace/file` caps **1 MiB**, same decoding, optional **`encoding`**. Paths in outputs are **relative to workspace** (POSIX), not host absolutes.
  - `copy_file` / `move_file`: Copy/move **files** inside workspace (path safety like `create_file`); default no overwrite unless `overwrite: true`; cross-device `move_file` copies then deletes source.
  - `read_dir` / `glob_files` / `list_tree`: List dir; glob recurse; tree with `max_depth` / `max_entries` caps, stay inside workspace.
  - `codebase_semantic_search`: **Semantic** code search (local **fastembed** + SQLite, separate from session long-term memory). **`rebuild_index: true`** scans `.gitignore`-aware tree, embeds chunks into **`.crabmate/codebase_semantic.sqlite`** (configurable path); **`query`** for cosine Top-K. Limits: **`codebase_semantic_*`** / **`AGENT_CODEBASE_SEMANTIC_*`**. Not read-only: `rebuild_index` **overwrites** index for the workspace key. Large repos: set **`path`** or narrow extensions. Disable tool: `codebase_semantic_search_enabled = false`.
  - `markdown_check_links`: Scan Markdown (default `README.md`, `docs/`) for relative links and `#fragment` anchors; `output_format=text|json|sarif`. External `http(s)://` default offline; optional `allowed_external_prefixes` enables HEAD probes (deduped cache).
  - `typos_check` / `codespell_check`: Spell check (read-only; needs [typos](https://github.com/crate-ci/typos) / [codespell](https://github.com/codespell-project/codespell)); default `README.md` + `docs/` if present; `paths` narrows; `typos_check` `config_path`; `codespell_check` `dictionary_paths` / `ignore_words_list`.
  - `ast_grep_run`: AST search with [ast-grep](https://ast-grep.github.io/) (install `ast-grep`); requires `pattern`, `lang`; default search under `src` excluding `target`, `node_modules`, `.git`; optional `paths` / `globs`.
  - `ast_grep_rewrite`: `ast-grep run --rewrite`; default `dry_run=true`; `dry_run=false` needs `confirm=true` (like `--update-all`).
  - `structured_validate` / `structured_query` / `structured_diff` / `structured_patch`: Validate/query/diff JSON·YAML·TOML; `structured_patch` point `set/remove` (default dry-run; write needs `confirm=true`). CSV/TSV validate/query/diff only—no structured write-back.
  - `table_text`: Preview/validate/filter/aggregate delimiter tables (CSV/TSV/; / |, streaming, 4 MiB/file); complements `structured_*` “load whole table as JSON”.
  - `text_transform`: In-memory transforms (Base64, URL encode/decode, short hash, line merge/split); no disk; length caps.
  - `text_diff`: Line unified diff for two UTF-8 strings or two workspace files (not Git); complements `structured_diff`.
  - `changelog_draft`: **git log** → Markdown changelog draft (no repo write); by date, `flat`, or adjacent **tag** ranges (`tag_ranges`).
  - `license_notice`: **cargo metadata** → Markdown **crate → license** table (placeholders if missing); **not legal advice**.
  - `repo_overview_sweep`: Read-only **rollup**: optional **project profile** (same as sidebar / `GET /workspace/profile` / first-turn inject: `Cargo.toml` / workspace, `package.json`, top dirs, tokei, optional `cargo metadata --no-deps`; `include_project_profile`, `project_profile_max_chars`, default on, 6000 cap); doc previews (default `README.md`, `AGENTS.md`, `docs/DEVELOPMENT.md`, …); `list_tree` on `src` (`source_roots`); globs for manifests/CI; ends with an **outline for conclusions**. **No LLM inside**—the model writes analysis from this output. Options: `doc_paths`, `doc_preview_max_lines`, `list_tree_*`, `build_globs`, ….
  - `docs_health_sweep`: Doc previews + `typos_check` + `codespell_check` + `markdown_check_links`. Missing CLIs → **skipped** steps. **External links**: only if **`md_allowed_external_prefixes`** non-empty does `markdown_check_links` use the **built-in HTTP client** for HEAD; **not** `http_fetch`/`http_request`, **not** `http_fetch_allowed_prefixes`, **no** Web/CLI approval. Empty prefix → count only, no network. Options: `fail_fast`, `summary_only`, `spell_paths`, per-step toggles, ….
  - `hash_file`: Read-only **SHA-256 / SHA-512 / BLAKE3** (streaming); optional `max_bytes` prefix hash.
  - `diagnostic_summary`: Redacted diagnostics—Rust toolchain (`rustc`/`cargo`/`rustup`/`bc`), workspace `target/`, common `Cargo.toml` / `frontend` paths, whether key env vars **are set** (**never values**; no length for secrets). Optional `extra_env_vars` (safe uppercase names).
  - `error_output_playbook`: Heuristic classification of **sanitized** rustc/cargo/npm/pytest errors → **2–3** suggested **`run_command`** strings (**not executed**; filtered to allowlist, e.g. `cargo`/`git`/`python3`/`npm`). `ecosystem`: `auto`/`rust`/`node`/`python`/`generic`; optional `max_chars`. Light redaction for `API_KEY=` patterns; still sanitize before paste.
  - `playbook_run_commands`: Same heuristics; **executes** **1–3** suggestions via internal **`run_command`** (same safety rules; may contend on cargo/npm locks). Same args as `error_output_playbook` + optional **`max_commands`** (default 3, max 3). **Trusted workspace**; on CLI tool failure a one-line JSON may be printed for the model to run this “diagnostic bundle”.
  - **Python / uv / pre-commit** (workspace root; CLIs must exist): `ruff_check`, `pytest_run` (`python3 -m pytest`), `mypy_check`, `python_install_editable` (uv or pip editable), `uv_sync`, `uv_run` (`args` array, no shell), `pre_commit_run` (needs `.pre-commit-config.yaml`). **Format**: `format_file` / `format_check_file` pick **ruff format** (`.py`), **clang-format** (C/C++ headers/sources), `rustfmt` / `prettier`, …. **Tag filtering**: `build_tools_with_options` + `dev_tag` labels (`python`, `cpp`, `go`, `jvm`, `docker`, `quality`, …)—see [`docs/en/DEVELOPMENT.md`](DEVELOPMENT.md). **Custom LLM backend**: `RunAgentTurnParams.llm_backend` optional `ChatCompletionsBackend` (default OpenAI-compatible HTTP).
  - **Node.js / npm** (needs `package.json`): `npm_install` (`npm ci`, `--production`), `npm_run` (any script; **`script` == `test`** can use test cache with **`[CrabMate test output cache hit]`**), `npx_run`, `tsc_check` (`tsc --noEmit`).
  - **Go** (needs `go.mod`, Go installed): `go_build`, `go_test` (`-run`/`-race`/`-timeout`, …), `go_vet`, `go_mod_tidy` (write needs `confirm`), `go_fmt_check` (`gofmt -l`), `golangci_lint`.
  - **JVM (Maven / Gradle)** (needs `mvn`/`gradle` or `gradlew`): `maven_compile` / `maven_test` (needs `pom.xml`, `mvn -q …`, optional `profile`/`test`), `gradle_compile` / `gradle_test` (needs `build.gradle*` or `settings.gradle*`, default tasks `classes`/`test`, conservative task validation). Like `run_command`: no `..` or absolute paths in args.
  - **Container CLI**: `docker_build` (**writes local images**; relative `context`/`dockerfile`/`tag`/`no_cache`), `docker_compose_ps` (read-only), `podman_images` (read-only). Host CLIs required; default allowlist includes `docker`/`podman`/`mvn`/`gradle` (`config/tools.toml`).
  - `chmod_file` (needs `confirm`, Unix): octal mode e.g. `755`.
  - `symlink_info` (read-only): target, dangling?, points outside workspace?.
  - **Process/port** (read-only): `port_check` (ss/lsof), `process_list` (ps filter).
  - **Metrics/analysis** (read-only): `code_stats` (tokei/cloc/fallback), `dependency_graph` (Cargo/Go/npm, Mermaid/DOT/tree), `coverage_report` (LCOV, Tarpaulin JSON, Cobertura XML).
  - **Source analysis** (read-only; CLIs required):
    - `shellcheck_check`: [ShellCheck](https://www.shellcheck.net/) for shell scripts; `paths`, `severity`, `shell`, `format`.
    - `cppcheck_analyze`: [cppcheck](https://cppcheck.sourceforge.io/) for C/C++; `paths`, `enable`, `std`, `platform`.
    - `semgrep_scan`: [Semgrep](https://semgrep.dev/) SAST; `config`, `paths`, `severity`, `lang`, `json`.
    - `hadolint_check`: [Hadolint](https://github.com/hadolint/hadolint) for Dockerfile; `path`, `format`, `ignore`, `trusted_registries`.
    - `bandit_scan`: [Bandit](https://bandit.readthedocs.io/) for Python; `paths`, `severity`, `confidence`, `skip`, `format`.
    - `lizard_complexity`: [lizard](https://github.com/terryyin/lizard) complexity; `paths`, `threshold`, `language`, `sort`, `warnings_only`, `exclude`.

## Rust dev tool examples

Structured function-calling JSON examples:

- `cargo_run`:
  ```json
  {"bin":"crabmate","args":["--help"]}
  ```
- `rust_test_one`:
  ```json
  {"test_name":"tools::tests::test_build_tools_names","nocapture":true}
  ```
- `cargo_audit`:
  ```json
  {"deny_warnings":true}
  ```
- `ci_pipeline_local` (local CI; optional Python `run_ruff_check` / `run_pytest` / `run_mypy`):
  ```json
  {"run_fmt":true,"run_clippy":true,"run_test":true,"run_frontend_lint":true,"run_ruff_check":true,"run_pytest":false,"run_mypy":false,"fail_fast":true,"summary_only":false}
  ```
- `release_ready_check`:
  ```json
  {"run_ci":true,"run_audit":true,"run_deny":true,"require_clean_worktree":true,"fail_fast":true,"summary_only":true}
  ```
- `cargo_nextest`:
  ```json
  {"profile":"default","test_filter":"tools::","nocapture":false}
  ```
- `cargo_fmt_check`:
  ```json
  {}
  ```
- `cargo_outdated`:
  ```json
  {"workspace":true,"depth":2}
  ```
- `cargo_machete` (unused-deps heuristic; `cargo install cargo-machete`):
  ```json
  {"with_metadata":false}
  ```
- `cargo_udeps` (unused-deps build check; `cargo install cargo-udeps`; often **`nightly: true`**):
  ```json
  {"nightly":true}
  ```
- `cargo_publish_dry_run` (does **not** upload):
  ```json
  {"package":"my-crate","allow_dirty":false,"no_verify":false}
  ```
- `rust_compiler_json` (`cargo check --message-format=json` diagnostics):
  ```json
  {"all_targets":true,"max_diagnostics":80,"message_format":"json"}
  ```
- `rust_analyzer_goto_definition` / `rust_analyzer_find_references` / `rust_analyzer_hover` (**`rust-analyzer` in PATH**; **0-based** line/char):
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"wait_after_open_ms":500}
  ```
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"include_declaration":true}
  ```
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4}
  ```
- `rust_analyzer_document_symbol`:
  ```json
  {"path":"src/lib.rs","max_symbols":200}
  ```
- `cargo_fix` (controlled write):
  ```json
  {"confirm":true,"broken_code":false}
  ```
- `cargo_deny`:
  ```json
  {"checks":"advisories licenses bans sources","all_features":true}
  ```
- `rust_backtrace_analyze`:
  ```json
  {"backtrace":"thread 'main' panicked at src/main.rs:10:5\nstack backtrace:\n   0: ...","crate_hint":"crabmate"}
  ```
- `frontend_lint`:
  ```json
  {}
  ```
- `frontend_build`:
  ```json
  {"script":"build"}
  ```
- `frontend_test`:
  ```json
  {"script":"test"}
  ```
- `workflow_execute` (DAG: parallelism, approval, SLA, compensation):
  - Node **`max_retries`** (0–5, default 0): auto backoff retry for **`timeout`**, **`workflow_tool_join_error`**, **`workflow_semaphore_closed`**, …; **not** for business failures (tests, non-zero exit) to avoid duplicate side effects.
  - **Static check**: each **`tool_name`** must be a built-in tool; **`tool_args`** must include schema **`required`** keys (recursive into nested objects/arrays; full validation still in runners).
  - **Result JSON** (`workflow_execute_result` / `workflow_validate_result`): **`workflow_run_id`** (matches logs), **`trace`** (`dag_start`, `node_attempt_*`, `node_retry_backoff`, `dag_end`, …), **`completion_order`**, **`nodes[].attempt`** final count.
  ```json
  {"workflow":{
    "max_parallelism":2,
    "fail_fast":true,
    "compensate_on_failure":true,
    "nodes":[
      {"id":"clean","tool_name":"cargo_clean","tool_args":{"dry_run":true},"deps":[],"compensate_with":[]},
      {"id":"clippy","tool_name":"cargo_clippy","tool_args":{"all_targets":true},"deps":["clean"],"compensate_with":["clean"]},
      {"id":"test","tool_name":"cargo_test","tool_args":{},"deps":["clippy"],"compensate_with":[]},
      {"id":"deny","tool_name":"cargo_deny","tool_args":{"checks":"advisories licenses bans sources","all_features":true},"deps":["test"],"requires_approval":true,"compensate_with":["clean"]}
    ]
  }}
  ```
 - `workflow_execute` (Git: inject `git_log` hash into `git_show`):
   ```json
   {"workflow":{
     "max_parallelism":2,
     "fail_fast":true,
     "compensate_on_failure":false,
     "nodes":[
       {"id":"log","tool_name":"git_log","tool_args":{"max_count":1,"oneline":true},"deps":[],"compensate_with":[]},
       {"id":"show","tool_name":"git_show","tool_args":{"rev":"{{log.stdout_first_token}}"},"deps":["log"],"compensate_with":[]}
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
       {"id":"diff","tool_name":"git_diff_base","tool_args":{"base":"main","context_lines":3},"deps":[],"compensate_with":[]},
       {"id":"patch_check","tool_name":"git_apply","tool_args":{"patch_path":"patches/fix.diff","check_only":true},"deps":["diff"],"compensate_with":[]},
       {"id":"patch_apply","tool_name":"git_apply","tool_args":{"patch_path":"patches/fix.diff","check_only":false},"deps":["patch_check"],"requires_approval":true,"compensate_with":[]}
     ]
   }}
   ```

   `patch_path` must point to an existing workspace patch (e.g. `patches/fix.diff`) you or a prior step created.

  Downstream string fields may use placeholders (recursive into JSON objects/arrays):
  - `{{node_id.output}}`: full output from `node_id` (truncated to `output_inject_max_chars`, default `2000`)
  - `{{node_id.status}}`: `passed`/`failed`
  - `{{node_id.stdout_first_line}}`: first line (truncated)
  - `{{node_id.stdout_first_token}}`: first token of first line (e.g. `git log --oneline` hash)

### Release-check templates

- `release_ready_check` (fast local loop):
  ```json
  {"run_ci":true,"run_audit":false,"run_deny":false,"require_clean_worktree":false,"fail_fast":true,"summary_only":true}
  ```

- `release_ready_check` (strict pre-release):
  ```json
  {"run_ci":true,"run_audit":true,"run_deny":true,"require_clean_worktree":true,"fail_fast":true,"summary_only":false}
  ```
- `cargo_tree`:
  ```json
  {"package":"crabmate","depth":2}
  ```
- `cargo_clean` (default dry-run preview):
  ```json
  {"release":true,"dry_run":true}
  ```
- `cargo_doc`:
  ```json
  {"package":"crabmate","no_deps":true,"open":false}
  ```

Also: `cargo_check`, `cargo_test` (cache with `rust_test_one` when enabled), `cargo_clippy`, `cargo_metadata`, `cargo_machete`, `cargo_udeps`, `cargo_publish_dry_run`, `rust_compiler_json`, rust-analyzer tools, `read_binary_meta`, `frontend_lint`, `find_references`, `rust_file_outline`, `format_check_file`, `quality_workspace`, `markdown_check_links`, `structured_*`, `table_text`, `text_diff`, `ast_grep_rewrite`, `diagnostic_summary`, `error_output_playbook`, `playbook_run_commands`, `package_query`, `cargo_tree`, `cargo_clean`, `cargo_doc`.

**Python / uv / pre-commit**: `ruff_check`, `pytest_run`, `mypy_check`, `python_install_editable`, `uv_sync`, `uv_run`, `pre_commit_run`; aggregates `run_lints`, `quality_workspace` (optional ruff/pytest/mypy, plus optional **`run_maven_*` / `run_gradle_*` / `run_docker_compose_ps` / `run_podman_images`**).

**Node.js / npm**: `npm_install`, `npm_run`, `npx_run`, `tsc_check`.

**Go**: `go_build`, `go_test`, `go_vet`, `go_mod_tidy`, `go_fmt_check`, `golangci_lint`.

**Process/port**: `port_check`, `process_list`.

**Git writes**: `git_checkout`, `git_branch_create`/`git_branch_delete`, `git_push`, `git_merge`, `git_rebase`, `git_stash`, `git_tag`, `git_reset`, `git_cherry_pick`, `git_revert`.

**Metrics**: `code_stats`, `dependency_graph`, `coverage_report`.

**Source analysis**: `shellcheck_check`, `cppcheck_analyze`, `semgrep_scan`, `hadolint_check`, `bandit_scan`, `lizard_complexity`.

### Python & pre-commit examples

- `ruff_check`:
  ```json
  {}
  ```
- `pytest_run`:
  ```json
  {"test_path":"tests","quiet":true}
  ```
- `uv_sync`:
  ```json
  {"frozen":false,"no_dev":false}
  ```
- `uv_run` (args are argv tokens, no shell):
  ```json
  {"args":["pytest","-q"]}
  ```
- `pre_commit_run` (default: staged files; combine with `all_files` / `files`):
  ```json
  {"all_files":true}
  ```

## Git tool examples

- `git_clean_check`:
  ```json
  {}
  ```
- `git_diff_stat`:
  ```json
  {"mode":"working"}
  ```
- `git_diff_names`:
  ```json
  {"mode":"working"}
  ```
- `git_fetch`:
  ```json
  {"remote":"origin","branch":"main","prune":true}
  ```
- `git_remote_list`:
  ```json
  {}
  ```
- `git_remote_set_url`:
  ```json
  {"name":"origin","url":"git@github.com:your-org/your-repo.git","confirm":true}
  ```
- `git_apply` (check first):
  ```json
  {"patch_path":"patches/fix.diff","check_only":true}
  ```
- `git_clone` (into workspace, needs confirm):
  ```json
  {"repo_url":"https://github.com/rust-lang/cargo.git","target_dir":"vendor/cargo","depth":1,"confirm":true}
  ```

## Common failure handling

Typical `release_ready_check` / `cargo_deny` / `cargo_audit` issues:

- `cargo_deny` (license/policy):
  ```bash
  cargo deny check advisories licenses bans sources
  ```
  Causes: `bans` hits, license allowlist, `sources` policy. Fix: upgrade/replace deps; adjust `deny.toml` with review; time-bound exceptions.

- `cargo_audit` (known vulnerabilities):
  ```bash
  cargo audit
  ```
  Causes: RustSec advisories, stale lockfile. Fix: `cargo update` / targeted updates; if upstream unfixed, mitigate; re-run audit + tests.

- Dirty tree (`require_clean_worktree=true`): `git_clean_check` fails—inspect `git status` / `git diff`; commit, stash, or set `require_clean_worktree=false` for quick local runs.

- Missing tools:
  ```bash
  cargo install cargo-deny
  cargo install cargo-audit
  ```

### Suggested pre-release order

1. Fast `release_ready_check` to smoke-test.
2. Strict `release_ready_check` after fixes.
3. Clean tree + key tests.
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
  Use **`encoding`** for legacy/BOM/unknown: `gb18030`, `utf-8-sig`, `auto`, …. Default **UTF-8 strict** (errors on invalid bytes). Response hints next `start_line`. `"count_total_lines": true` rescans for totals. `start_line`+`end_line` still respect `max_lines`.
- `modify_file` (large files, `replace_lines`):
  ```json
  {"path":"src/huge.rs","mode":"replace_lines","start_line":120,"end_line":135,"content":"// new chunk\n"}
  ```
- `read_dir`:
  ```json
  {"path":"src","max_entries":50,"include_hidden":false}
  ```
- `file_exists`:
  ```json
  {"path":"src/main.rs","kind":"file"}
  ```
- `extract_in_file` (regex lines; optional **`encoding`**):
  ```json
  {"path":"src/main.rs","pattern":"workflow_execute","max_matches":20,"case_insensitive":true}
  ```
  Rust function block mode:
  ```json
  {"path":"src/main.rs","pattern":"pub\\s+fn\\s+run_agent_turn","mode":"rust_fn_block","max_matches":1}
  ```
- `markdown_check_links`:
  ```json
  {"roots":["README.md","docs"],"max_files":300,"check_fragments":true,"output_format":"text","allowed_external_prefixes":["https://example.com/docs/"],"external_timeout_secs":10}
  ```
  Default roots: `README.md` and `docs/`.
- `structured_validate`:
  ```json
  {"path":"frontend-leptos/Cargo.toml","format":"auto","summarize":true}
  ```
- `structured_query`:
  ```json
  {"path":"Cargo.toml","query":"/package/name","format":"toml"}
  ```
  Dot path: `{"path":"frontend-leptos/Cargo.toml","query":"dependencies.leptos"}`.
- `structured_diff`:
  ```json
  {"path_a":"specs/openapi.v1.json","path_b":"specs/openapi.v2.json","max_diff_lines":200}
  ```
  **CSV/TSV**: `format` `csv`/`tsv` or by extension; `has_header=false` → array-of-arrays; query example `{"path":"data/sample.csv","query":"/0/name"}`.
- `table_text`: preview `{"action":"preview","path":"data/foo.tsv","preview_rows":15}`; filter `{"action":"filter_rows","path":"data/foo.csv","column":2,"contains":"ERR","max_output_rows":100}`.
- `diagnostic_summary` (**no env values**):
  ```json
  {"include_toolchain":true,"include_workspace_paths":true,"include_env":true,"extra_env_vars":["CI"]}
  ```
- `codebase_semantic_search` (fastembed/ONNX; same stack as long-term memory):
  - Rebuild (query optional):
  ```json
  {"rebuild_index": true, "path": "src"}
  ```
  - Query:
  ```json
  {"query": "Where is chat job queue concurrency configured?", "top_k": 6}
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
- `find_symbol`:
  ```json
  {"symbol":"run_agent_turn","kind":"fn","path":"src","context_lines":2,"max_results":10}
  ```
- `find_references`:
  ```json
  {"symbol":"run_tool","path":"src/tools","max_results":50,"case_sensitive":false,"exclude_definitions":true}
  ```
- `rust_file_outline`:
  ```json
  {"path":"src/tools/mod.rs","include_use":false,"max_items":200}
  ```
- `format_check_file`:
  ```json
  {"path":"src/main.rs"}
  ```
- `quality_workspace` (default fmt check + clippy; optional test / frontend lint / prettier):
  ```json
  {"run_cargo_fmt_check":true,"run_cargo_clippy":true,"run_cargo_test":false,"run_frontend_lint":false,"run_frontend_prettier_check":false,"fail_fast":true,"summary_only":false}
  ```
