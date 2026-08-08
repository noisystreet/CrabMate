# Changelog

All notable changes to **CrabMate** (this server repository) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

中文说明：本文件以英文为主（与默认 [`README.md`](README.md) 一致）；面向使用者的中文入口见 [`README.zh.md`](README.zh.md)。发版时先在 **`[Unreleased]`** 积累条目，打 tag 前再切到版本分区。

## [Unreleased]

### Added

- GitHub Actions **Release** workflow (`.github/workflows/release.yml`): tag `vX.Y.Z` (or `vX.Y.Z-rc.N`) → `make package` + GitHub Release with tar.gz/deb; notes from this file’s core `X.Y.Z` section; re-run updates the same Release.

### Changed

- (none yet)

### Fixed

- (none yet)

## [0.1.0] - 2026-08-08

First public **server** release tag (`v0.1.0`). Cargo package version was already `0.1.0`; this changelog marks the cut for GitHub Release / installable artifacts.

**Scope**: this repo is the Agent **server** (HTTP API, CLI/REPL/TUI, tools, SSE). Official Web UI and desktop/Android shells live in the sibling Client repo [`crabmate-client`](https://github.com/noisystreet/crabmate-client) (path A, Phase 4.2 complete).

### Added

- OpenAI-compatible `chat/completions` client (DeepSeek, MiniMax, Zhipu GLM, Moonshot Kimi, Ollama, …) with streaming, retries, and tool calling.
- HTTP **`serve`**: `/chat`, `/chat/stream` (SSE / AG-UI), workspace APIs, conversation SQLite under `.crabmate/`, optional Web API Bearer.
- Built-in tool registry (`run_command` allowlist, file tools, fetch, cargo/npm stacks, workflows, optional MCP / Docker sandbox / fastembed via Cargo features).
- CLI: `serve`, `repl`, `tui`, `chat`, `doctor`, `models` / `probe`, `save-session`, `mcp`, packaging helpers.
- Client contract versioning gates (`client-contract-v*`) and CI smoke for SSE / OpenAPI / consumer pins.
- Release packaging: `make package` → server-only **tar.gz** + **`.deb`**; **systemd** unit (`crabmate.service`), `/etc/crabmate/config.toml` + `config/prompts/`, env example (`KEY=value` only).
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
- Default Cargo features include **`mcp`** (among `web` / `repl` / `tui`); heavy options such as **`fastembed`** remain opt-in.
- Systemd service user has a **minimal `PATH`**; extend via `/etc/crabmate/crabmate.env` for host toolchains. Bypass HTTP proxies for `127.0.0.1` when probing locally.
- Compatibility-layer shrink items **B2–B4**, full unwrap audits, and agent benchmarks remain backlog ([`docs/待办清单.md`](docs/待办清单.md)).

[Unreleased]: https://github.com/noisystreet/CrabMate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/noisystreet/CrabMate/releases/tag/v0.1.0
