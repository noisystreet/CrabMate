# AGENTS.md

> 本文件是给 **AI coding agent** 的契约摘要，**不绑定任何具体 agent 工具**——任何能读仓库的 agent 都按本文件执行。
> 只放三样东西：**硬红线**、**每轮必跑的检查**、**去哪查**。细则在 `docs/`，见文末「文档索引」。
> 与本文件冲突的说法以本文件为准，然后顺手把对应 docs 改对。**不要另建平行规则文件。**

## 1. 项目概览 / Project overview

CrabMate is a Rust-based AI Agent that calls **OpenAI-compatible** `chat/completions` backends (e.g. **DeepSeek**, **MiniMax**,
  **Zhipu GLM**, **Moonshot Kimi**, **Ollama**). It provides HTTP **`serve`** (Axum) and ops CLIs; the **official Web UI /
  Desktop / remote terminal (`crabmate-tui`)** live in **[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**
  (local sibling `../crabmate-client`; Playwright forwarding honors **`CRABMATE_CLIENT_DIR`**). In-process **`repl` / `chat` /
  `tui`** are **removed** (D2.2; see `docs/design/client_shell_split.md`).

入口文档：`README.md`（快速上手）· `docs/配置说明.md`（env / TOML，**供应商细节唯一权威源**）·
  `docs/命令行与路由.md`（子命令 / 路由）· `docs/工具说明.md`（内置工具）· `docs/开发文档.md`（架构）。

## 2. 工作区约定 / Workspace

- 临时文档、分析报告、设计草稿一律放 **`agent_space/`**（已 gitignore，不入库），以隔离中间产物、保持根目录与 `docs/` 干净。
- **Do not cite, link, or treat `agent_space/` files as project documentation.** 权威文档在 `docs/`（及根 `README*` /
  `AGENTS.md`）。草稿成熟后毕业进 `docs/`（或 `docs/design/`）并让读者指向那里——不要在入库文档、代码注释、PR 正文或回复里
  留下「见 `agent_space/…`」这类长期引用。

## 3. 运行与环境事实 / Running & environment

- **`cargo run -- serve` 永远纯 API**，不托管 SPA；UI 由 Client 仓托管并经 CORS 接入（**`CM_WEB_CORS_ALLOWED_ORIGINS`**）。
  默认端口 8080，只绑定 **127.0.0.1**。
- 默认 `llm_http_auth_mode = bearer`：**`serve` / `bench` 可在未设置 `API_KEY` 时启动**，真正调用 `chat/completions` 前才需要
  （优先级：请求体 `client_llm.api_key` → 环境变量 `API_KEY`）。本地 Ollama 等请设 **`llm_http_auth_mode = "none"`**。
- `serve --host 0.0.0.0`（非 loopback）必须配 **`CM_WEB_API_BEARER_TOKEN`** 或
  **`CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`**，否则拒绝启动。
- **`POST /config/reload`** 热重载大部分 `AgentConfig`（**`API_KEY` 不从环境重读**）。
- 构建需 **Rust 1.85+**（edition 2024）；`--features fastembed` 另需 `g++` / libstdc++。
- 配置项与供应商开关只查 `docs/配置说明.md`；缺失 CLI、`/health` degraded、Web Bearer 排障见
  `docs/调试指南.md`「环境与工具链陷阱」。

## 4. 标准命令 / Lint · Test · Build

| 任务 | 命令 |
| --- | --- |
| 构建 | `cargo build` |
| 测试 | `cargo test`（nightly：`cargo +nightly test`） |
| clippy | `cargo clippy` |
| 格式检查 | `cargo fmt --check` |
| 依赖漏洞（RustSec） | `cargo audit` |
| 依赖许可证 / 来源 | `cargo deny check licenses bans sources`（配置 `deny.toml`） |
| 全量测试 | `make test`（`cargo test --workspace`） |
| 打包 | `make package` → `dist/` |

man 页再生成、前端构建等其余命令见 `README.md` / `docs/命令行与路由.md`。

## 5. 仓库工程规则 / Repository engineering rules

**本节是 agent 的唯一规则源。** 若你的工具从别的路径读规则，让它指向本文件，不要另建平行规则文件。

### 5.1 本地质量入口 / Local quality entry points

三个入口**不可互换**——「本地绿、CI 红」多半是跑错了：

| When | Command | Covers |
|------|---------|--------|
| Before every commit | `pre-commit run --all-files` | `cargo fmt`, `cargo clippy -D warnings`, `cargo doc` (rustdoc warnings denied), crate-dep policy, lizard CCN, fn param/nloc ratchets, builtin tool-name literal ratchet, version sync (`scripts/check-version-sync.sh`), markdown line-length ratchet (`scripts/md-line-length-ratchet.sh`), `taplo` |
| Build / package | `make backend` / `make package` (see `make help`) | Compilation and release artifacts; **not** a gate |
| Full test suite | `make test` (i.e. `cargo test --workspace`) | Also run in CI |

- **`pre-commit` 故意不跑 `cargo test`**（golden 也不跑）：所以「pre-commit 通过」永远不等于「测试通过」。
- 不要为了对齐 CI 往 pre-commit 里塞慢检查；贵的 job 留给 CI，提交钩子保持快。

### 5.2 对外写操作 / Outbound writes (repository-visible actions)

Acting on the remote in a way other people can see. Local side effects are governed by `tool_approval` / `permission_level`;
  this section governs repository actions. The two are independent — a tool-approval allowlist is **not** a governance mandate,
  and this section never relaxes a tool check.

- **May do without asking** (read-only or local-only): `gh issue view` / `search`, `gh issue list`, `gh pr view` / `list` /
  `checks` / `diff`, `git log` / `diff` / `blame` / `show`, creating a local branch, committing on that branch.
- **One explicit human approval per action** — state the exact command and its target, then wait: `gh issue close` / `reopen` /
  `comment` / `edit`, adding or removing labels, `gh pr merge`, `gh release create` / `edit`, pushing a branch (the
  `--force-with-lease` case below is a subset and needs the same approval), `gh pr edit --base`, deleting a branch.
- **Only when a human asks for that exact act**: push to `main`, force-push a shared branch, `gh pr merge --admin`,
  closing an Issue that the current work does not deliver.

Notes on reading this list:

- A blanket approval — “go ahead with everything”, a Web `AllowAlways` on `run_command` — does **not** cover it.
- A task that itself demands the act (“fix it and open a PR”) is that one approval for `gh pr create` on that branch;
  it is **not** approval to merge, comment on, or close anything afterwards.
- Not sure whether an act is repository-visible? It is — ask.

### 5.3 提交与推送 / Commit & push

- Before **every** `git commit`: `pre-commit run --all-files`. 若未安装 pre-commit，至少在仓库根跑：`cargo fmt --all`；
  `cargo clippy --all-targets --all-features -- -D warnings`；`bash scripts/lizard-rust.sh`（CCN ≤ 10）；
  `bash scripts/fn-param-ratchet.sh`（参数 ≤ 9）；`bash scripts/fn-nloc-ratchet.sh`（fn nloc ≤ 200、文件 ≤ 920 行）；
  `bash scripts/tool-name-literal-ratchet.sh`（不在比较 / match / or-pattern 处新增工具名字面量，用 `crate::cm_names::*`，见 `#1155`）；
  `bash scripts/md-line-length-ratchet.sh`（markdown 不新增超 160 显示列的行——折行，不要重生成基线）。
- 钩子失败就修到全绿；钩子改写过的文件要重新 `git add` 再提交。**`--no-verify` 严格禁止**——若用户明确要求例外，必须在回复中说明风险。
- **Subject format**: `type(scope): 中文简述 / English brief` — Conventional Commits + 双语，`scope` **只写一次**且用英文，
  两侧都以动词开头、语义对称。不兼容变更用 `!` 或 `BREAKING CHANGE:` footer。裸消息（「更新」「修改」）会被 `commit-msg` 钩子拒绝。
- **Before `git push`**: `git fetch origin main` → `git rebase origin/main` → 解冲突（`git add` + `git rebase --continue`）
  → 重跑上面的检查 → push。rebase 需要的 `--force-with-lease` 属对外写操作，须显式授权（见 5.2）。
- **`cargo test` 不在 pre-commit 里**，自己按范围选：

| Change | Test scope |
|--------|-----------|
| Single module / file | `cargo test -p crabmate <filter>` or just the related tests |
| `Cargo.toml`, workspace, cross-module types, `src/lib.rs` / public API, protocol / SSE / config loading | full `cargo test` |
| Pre-merge, release, CI alignment | full `cargo test` |

- 新增逻辑若没有测试覆盖：尽量补单元或最小集成测试；补不了就在 PR / 回复里说明风险与计划。

### 5.4 Rust 错误处理 / Rust error handling

- **生产路径禁止裸 `unwrap()` / `expect("…")`**，优先 `?` + 上下文（`.map_err` 带解释）。
- `expect` 只允许在消息里写明不变量之处：`src/main.rs` 启动期初始化（本就该中止进程）、一次性 `LazyLock` / `OnceLock` 初始化。
  `#[cfg(test)]` / `tests/` / `benches/` 可自由使用，但失败信息要可读。
- **`unsafe`：默认不写。** 不得已时——范围最小，紧贴块上方注释说明*为什么成立*（不变量、前置条件、绑定的 FFI / 库契约），
  不要在多个地方复制同一段 unsafe 而不做封装。
- `panic!` 只用于不可达分支，绝不用于可预期的 IO / 解析失败（返回 `Result`）。

### 5.5 密钥与日志 / Secrets & logging

- 绝不把真实 **API key、token、私钥、数据库口令** 写进 `src/`、`config/*.toml`、fixtures、脚本、**commit message**、
  PR / Issue 正文，或看起来像真值的注释。
- `.env` / `.env.*`：只留本地（gitignore）；**不要**把内容粘贴进对话、文档、补丁或截图。示例用占位符（`YOUR_DEEPSEEK_API_KEY`），
  任何 `sk-…` 样例都必须标明是虚构值。
- 真实密钥一旦进过提交：**立刻轮换**并改写历史——单纯 `git revert` 不够。
- `tracing` / `log` / `eprintln!` / 返回给客户端的错误串：绝不输出完整密钥、整条 `Authorization: Bearer …`、或带真实
  `?key=` / `?token=` 的 URL；只记*是否设置*、长度或少量首尾字符（或用共享 redact helper）。
- 工具入参、子进程 stdout、第三方响应体都可能含凭据——落盘或写日志前截断 / 脱敏；面向用户的文案保持笼统（如「API 密钥无效」）。
- 审查 diff 时检查新增的形似密钥的常量，以及把 `std::env` / `process.env` 值直接拼进日志的写法。

### 5.6 安全敏感面 / Security-sensitive surfaces

动到以下任一处，必须显式检查边界，并在 PR / 回复里说明风险与缓解：

| Area | Entry points | Focus |
|------|--------------|-------|
| Workspace switching & paths | `src/web/workspace/`, `src/workspace/path.rs`, `src/workspace/fs.rs`, `src/tools/file/path.rs` | Normalization, no unauthorized arbitrary directories, `..` / symlink handling; Linux already uses `openat2` `RESOLVE_IN_ROOT` on some paths — residual TOCTOU is documented in `path_workspace` / `workspace_fs` and must not be softened. |
| Command execution | `src/tools/command.rs`, allowlist config, `run_command` in `tool_registry` | Allowlisted commands only; `..` / absolute-path argument rejections stay as-is unless intentionally changed *and* documented. |
| HTTP fetching | `src/tools/http_fetch.rs`, `http_fetch` / `http_request`, `http_fetch_allowed_prefixes` | Conservative prefixes; approval semantics for unmatched URLs (including the `http_request` METHOD key) must match the docs. |
| File read/write tools | `src/tools/file.rs` and friends | `resolve_for_read` / `resolve_for_write` semantics must not be bypassed. |

**不要**通过弱化注释或文档里的安全说明来掩盖未缓解的路径；已知缺口记在 `docs/待办清单.md`。

### 5.7 架构与文档同步 / Architecture & documentation sync

- 改**架构**或 `src/` 模块组织 ⇒ 复核并更新 `docs/开发文档.md` 的「架构设计」（分层、Web 数据流、Mermaid 节点/边）与
  「主要模块」职责表。保持概览粒度——细节留在专题文档，不要在那边重新长出逐文件表（工具清单属于 `docs/工具说明.md` / 注册表）。
- 触发条件：`src/lib.rs` 增删 `mod`；重排 `Router` / `AppState`；新建 / 改名 / 删除 `src/` 文件或目录；某文件主职责变了；
  `tool_registry` 新增执行类别；`runtime/` / `web/` 子模块边界移动。
- 不需要：纯实现改动、私有改名、注释 / 格式、只动 `tests/` / `#[cfg(test)]` / fixtures。
- **用户可见变更**（CLI flag、API、配置键、TUI/Web 行为、工具）⇒ 同步 `README.md`（用法）与 `docs/工具说明.md`（工具）。
  改 `config/*.toml`、axum 路由/handler（路径、方法、请求/响应体、新端点）或增删 `CM_*` 环境变量 ⇒ **必须**同步 `README.md`
  与 `docs/开发文档.md`。
- `docs/待办清单.md`：条目实现后删除（不要留 `- [x]`；「何时完成」查 git 历史）；小节空了连标题一起删。

### 5.8 Chat / SSE 契约（两端）/ Chat / SSE contract

这些路径与概念是**耦合**的——改动时须同时核对 Rust 路由/handler、Client 解析器与文档：

- **路由与请求体**：`src/lib.rs` 的 `/chat`、`/chat/stream` 及其 JSON 字段。
- **SSE 线协议**：`src/cm_sse_protocol/`（根重导出 `crate::sse`）、`SSE_PROTOCOL_VERSION`、payload 类型、错误码、`data:` 分帧。
  权威表见 **`docs/SSE协议.md`**，对齐 Client 的 `frontend/src/api/chat_stream/parser_v2.rs`、`frontend/src/sse_dispatch/types.rs`。
- **`src/cm_sse_protocol/sse/line.rs`**（`classify_agent_sse_line`）故意没有生产消费者——它是单实现语义的锚点，
  **不要**当死代码清理；要么与 Web 消费者保持一致，要么在代码注释与 `docs/SSE协议.md` 两处写明有意分歧。
- 清单：字段增删改名 ⇒ serde 类型 + Client `parser_v2.rs` + `sse_dispatch/types.rs` + 协议文档；控制面分发顺序或新顶层键 ⇒
  `src/cm_sse_protocol/control_classify.rs`、`fixtures/sse_ag_ui_golden.jsonl`、Client `parser_v2.rs`，并跑
  `cargo test --lib golden_ag_ui_classify_matches_expected`（以及 Client 仓的 `golden_ag_ui_v2_parser_matches_expected`）。
  新错误码 ⇒ 前端展示 / 重试路径，且响应体不得含密钥。
- **本仓没有 `frontend/` 源码**：UI 检查在 Client 仓（`make frontend-check` / `make frontend`），契约钉 `client-contract-v*`。

### 5.9 工具注册 / Tool registry (`src/cm_tools/`)

- **Args 与 spec 同源**（#1157 已迁移，别再手抄 `ToolSpec` 的 `parameters` / 摘要结构）：`tool_param_types/` 写 Args
  （`Deserialize + JsonSchema`），`specs/*.inc.rs` 用 `builtin_tool!` 声明一行，执行体是 `runners.rs` 的 `runner_*` 薄包装。
- **`name` 必须引用 `src/cm_tools/tool_names.rs` 的常量（绝不写字面量），并同时加进 `tool_names::ALL`**——
  `all_tool_names_match_specs` 会拦住漏加。同步 `README.md` + `docs/工具说明.md`。
- 零漂移验证（不带 BLESS 跑绿才算没漂移）：
  `cargo test -p crabmate --lib cm_tools::tools::tool_specs_registry`（archive 三件套需 `--features archive-tools`）。
- 完整三步流程、`ToolArgsSummary` 宽容取值、`async_only` runner、golden 快照细节见
  **`docs/工具说明.md`「新增 / 重命名内置工具（三步）」**。

### 5.10 发布与依赖 / Release & dependencies

- 版本号**唯一来源**是 `Cargo.toml` → `[package] version`；改版本**只走 `make set-version VERSION=X.Y.Z[-pre]`**，
  它会连带重解析 `Cargo.lock`、重生成 `docs/openapi.json` 与 `man/*.1`，并打印建议的 CHANGELOG 标题行。不要手改。
- tag 格式 `vX.Y.Z`（可 `-rc.N`）；打 tag 前先把 `[Unreleased]` 收成 `## [X.Y.Z]` 段落；push tag 即发版。
- 破坏配置键 / SSE / HTTP / 公共 CLI 属**破坏性变更**：commit subject 标 `!` 或 `BREAKING CHANGE:`，并写进 CHANGELOG。
- 动 `Cargo.toml` / `Cargo.lock` 时核对新 crate 许可证（`deny.toml` + `cargo deny`）、`cargo audit`、锁文件一致性。
- 全流程、CHANGELOG 记什么 / 不记什么、许可证细节见 **`docs/发布流程.md`**。

### 5.11 CLI / TUI / Web 共享逻辑 / Shared logic

D2.2 之后同进程 `chat` / `repl` / `tui` 已硬删。现存面：本仓运维 CLI（`serve`、`doctor`、`web-bearer`、`save-session`、
  `tool-replay`）、Client **`crabmate-tui`**（HTTP + SSE）、Client Web / Desktop UI（本仓 `lib` 的 HTTP + SSE）。

- 每加一个能力，先判断它是否**天生只属于一个面**（Web-only 路由、TUI-only 快捷键、ops-only 子命令形态）。
- **不是**天生的：就让 ops CLI、`crabmate-tui`、Web 都能到达；差异只允许在交互与呈现。
- **是**天生的：可以只实现一个面，但**数据模型、工具执行与配置键仍属共享层**（`agent/`、契约类型），以便其余面接入。
- 与 UI 无关的规则放可复用模块（`agent/`、`types`、`sse::protocol`、`tool_registry`、`runtime/` 的非 UI 部分）；
  session / export 形状已在 `runtime/workspace_session`、`runtime/chat_export`，扩展同一套契约。

## 6. Git 与 GitHub 协作 / Git & GitHub flow

- 默认 **GitHub Flow**：每次改动从 `main` 拉短分支 → 开 PR → 检查通过且评审完成后回 `main`；`main` 始终可发布，
  不做 feature 直接 push。
- **先有 Issue**：新计划、设计、非平凡重构必须先有跟踪 Issue（问题 / 范围 / 明确非目标 / 引用的 `docs/` 锚点）；
  琐碎修复（typo、一行 bug）可跳过。设计草稿留在 `agent_space/`，Issue 定稿后毕业进 `docs/design/`。
- **分支命名** `type/short-slug`（`type` 同提交类型），有 Issue 就带编号（`feat/842-ltm-hybrid-retrieval`）；
  `main` 是唯一长期分支，合并后删分支。
- **PR 与 Issue 关联**：正文写 `Closes #123`（本 PR 完成）或 `Refs #123`（只推进，Issue 保持 open）；
  不要在 PR 里悄悄扩大范围，另开 follow-up Issue。
- 开 / 合 PR 前至少 `pre-commit run --all-files`，并确保 CI 绿；push 前先 rebase 到最新 `origin/main`。
- **Stacked PRs**（base 在未合并分支上）、rebase-only 仓库的 `CONFLICTING` 处理、`--force-with-lease=<ref>:<sha>` 显式写法
  与冲突复现步骤见 **`docs/git协作流程.md`**。

## 7. 文档索引 / Where to look

| 想知道 | 去哪 |
| --- | --- |
| 配置项 / 供应商 / 认证模式 / 热重载 | `docs/配置说明.md` |
| 子命令、HTTP 路由、日志级别、打包 | `docs/命令行与路由.md` |
| 内置工具清单、权限等级、工具注册三步 | `docs/工具说明.md` |
| 架构与主要模块、扩展点 | `docs/开发文档.md` |
| SSE 行协议与错误码 | `docs/SSE协议.md` |
| 环境陷阱、缺失 CLI、日志与探针、前端构建 | `docs/调试指南.md` |
| 测试与 CI 对齐、E2E | `docs/测试指南.md` |
| 发版、CHANGELOG、依赖许可 | `docs/发布流程.md` |
| 分支 / 提交 / Stacked PR 细则 | `docs/git协作流程.md` |
