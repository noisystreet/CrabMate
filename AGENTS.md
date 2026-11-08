# AGENTS.md

> 给 **AI coding agent** 的契约摘要，**不绑定任何具体 agent 工具**——任何能读仓库的 agent 都按本文件执行。
> 只放三样东西：**硬红线**、**每轮必跑的检查**、**去哪查**（见 §6）。细则在 `docs/`；冲突以本文件为准，然后顺手把 docs 改对。
> **不要另建平行规则文件。**

## 1. 项目概览 / Project overview

Rust 写的 AI Agent，调用 **OpenAI 兼容** `chat/completions` 后端（DeepSeek / MiniMax / 智谱 GLM / Moonshot Kimi / Ollama 等）；
本仓是 HTTP **`serve`**（Axum）+ 运维 CLI。**Web / Desktop / 远程终端（`crabmate-tui`）在
[`crabmate-client`](https://github.com/noisystreet/crabmate-client)**（本地同级 `../crabmate-client`；Playwright 转发认
`CRABMATE_CLIENT_DIR`）。同进程 **`repl` / `chat` / `tui` 已硬删**（D2.2，见 `docs/design/client_shell_split.md`）。
上手看 `README.md`，其余见 §6。

## 2. 工作区约定 / Workspace

- 临时文档、分析报告、设计草稿一律放 **`agent_space/`**（gitignore，不入库），保持根目录与 `docs/` 干净。
- **不要把 `agent_space/` 当项目文档**引用或链接；草稿成熟后毕业进 `docs/`（或 `docs/design/`）并让读者指向那里，
  不要在入库文件、代码注释、PR 正文里留下「见 agent_space/…」这类长期引用。

## 3. 运行与环境事实 / Running & environment

- **`cargo run -- serve` 永远纯 API**，不托管 SPA；UI 在 Client 仓经 CORS 接入（`CM_WEB_CORS_ALLOWED_ORIGINS`）。默认 `127.0.0.1:8080`。
- 默认 `llm_http_auth_mode = bearer`：**`serve` / `bench` 缺 `API_KEY` 也能启动**，真正调用 `chat/completions` 前才需要
  （优先级：请求体 `client_llm.api_key` → 环境变量 `API_KEY`）。本地 Ollama 等设 `llm_http_auth_mode = "none"`。
- `serve --host 0.0.0.0`（非 loopback）必须配 `CM_WEB_API_BEARER_TOKEN` 或
  `CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK=true`，否则拒绝启动。
- **`POST /config/reload`** 热重载大部分 `AgentConfig`（**`API_KEY` 不从环境重读**）。
- 构建需 **Rust 1.85+**（edition 2024）；`--features fastembed` 另需 `g++` / libstdc++。
- 配置项与供应商开关只查 `docs/配置说明.md`；缺失 CLI、`/health` degraded、Web Bearer 排障见
  `docs/调试指南.md`「环境与工具链陷阱」。

## 4. 仓库工程规则 / Repository engineering rules

**本节是 agent 的唯一规则源。** 若你的工具从别的路径读规则，让它指向本文件，不要另建平行规则文件。

### 4.1 质量门禁 / Quality gates

| 场景 | 命令 |
| --- | --- |
| 每次提交前（**必跑**） | `pre-commit run --all-files`（fmt / clippy `-D warnings` / `cargo doc` / 依赖策略 / CCN·参数·nloc·工具名字面量·md 行宽棘轮 / 版本同步 / `taplo`） |
| 构建 / 打包 | `make backend` / `make package` → `dist/`（**不是**门禁） |
| 全量测试 | `make test`（`cargo test --workspace`）；单项：`cargo test -p crabmate <filter>` |
| 单项检查 | `cargo build` · `cargo clippy` · `cargo fmt --check` · `cargo audit` · `cargo deny check licenses bans sources` |

- **`pre-commit` 故意不跑 `cargo test`**（golden 也不跑）：所以「pre-commit 通过」永远不等于「测试通过」。
- 不要为对齐 CI 往 pre-commit 塞慢检查；贵的 job 留给 CI，提交钩子保持快。
- 未装 pre-commit 时，至少在仓库根跑：`cargo fmt --all`；`cargo clippy --all-targets --all-features -- -D warnings`；
  `bash scripts/lizard-rust.sh`（CCN ≤ 10）；`bash scripts/fn-param-ratchet.sh`（参数 ≤ 9）；
  `bash scripts/fn-nloc-ratchet.sh`（fn ≤ 200 行 / 文件 ≤ 920 行）；`bash scripts/tool-name-literal-ratchet.sh`
  （比较 / match 处不新增工具名字面量，用 `crate::cm_names::*`，见 #1155）；`bash scripts/md-line-length-ratchet.sh`
  （md 不新增超 160 显示列的行——折行，不要重生成基线）。

### 4.2 对外写操作 / Outbound writes (repository-visible actions)

`tool_approval` / `permission_level` 管**工具执行**，本节管**仓库治理动作**——两者独立，审批白名单不构成治理授权。

| 档位 | 内容 |
| --- | --- |
| **免确认**（只读 / 仅本地） | `gh issue view`·`search`·`list`、`gh pr view`·`list`·`checks`·`diff`、`git log`·`diff`·`blame`·`show`、建本地分支、在该分支提交 |
| **逐次人工批准**（先说清命令与目标再动手） | `gh issue close`·`reopen`·`comment`·`edit`、增删标签、`gh pr merge`、`gh release create`·`edit`、push 分支（含 `--force-with-lease`）、`gh pr edit --base`、删除分支 |
| **仅当明确要求那件事** | push 到 `main`、强推共享分支、`gh pr merge --admin`、关闭本次工作未交付的 Issue |

- 「全都批准」/ Web `AllowAlways` **不覆盖**本表；本节也不放宽任何工具检查。
- 任务本身要求某个动作（如「修好并开 PR」）= 该分支 `gh pr create` 的那一次授权，**不含**之后的 merge / 评论 / 关闭。
- 拿不准某动作是否对外可见？是——先问。

### 4.3 提交与推送 / Commit & push

- **Subject**：`type(scope): 中文简述 / English brief`——Conventional Commits + 双语，`scope` 只写一次且用英文，两侧动词开头、
  语义对称。破坏性变更用 `!` 或 `BREAKING CHANGE:` footer。裸消息（「更新」「修改」）会被 `commit-msg` 钩子拒绝。
- **`--no-verify` 严格禁止**；钩子失败就修到全绿，钩子改写过的文件重新 `git add`。用户坚持要例外时，必须在回复里说明风险。
- **push 前**：`git fetch origin main` → `git rebase origin/main` → 解冲突（`git add` + `git rebase --continue`）→ 重跑检查 → push。
- **`cargo test` 范围自选**：单模块 / 单文件 → `cargo test -p crabmate <filter>`；动 `Cargo.toml` / workspace / 跨模块类型 /
  `src/lib.rs` 或公共 API / 协议·SSE·配置加载 → 全量；合并前 · 发版 · 对齐 CI → 全量。
- 新增逻辑无测试覆盖：尽量补最小测试；补不了就在 PR / 回复里说明风险与计划。

### 4.4 Rust 错误处理 / Rust error handling

- 生产路径**禁止裸 `unwrap()` / `expect("…")`**，用 `?` + 上下文（`.map_err` 带解释）。
- `expect` 仅限消息里写明不变量处：`src/main.rs` 启动期初始化、一次性 `LazyLock` / `OnceLock`。
  `#[cfg(test)]` / `tests/` / `benches/` 可自由用，但失败信息要可读。
- **`unsafe` 默认不写**；不得已时范围最小，紧贴块上方注释说明*为什么成立*（不变量、前置条件、绑定的 FFI / 库契约），
  不要在多处复制同一段而不封装。
- `panic!` 只用于不可达分支，绝不用于可预期的 IO / 解析失败（返回 `Result`）。

### 4.5 密钥与日志 / Secrets & logging

- 真实 **API key / token / 私钥 / 数据库口令** 绝不进 `src/`、`config/*.toml`、fixtures、脚本、commit message、PR / Issue 正文。
- `.env*` 只留本地；**不要把内容**粘贴进对话、文档、补丁或截图。示例用占位符，`sk-…` 必须标明是虚构值。
- 密钥一旦进过提交：**立刻轮换**并改写历史——单纯 `git revert` 不够。
- `tracing` / `log` / `eprintln!` / 返回给客户端的错误串：不输出完整密钥、整条 `Authorization: Bearer …`、或带真实
  `?key=` / `?token=` 的 URL；只记*是否设置* / 长度 / 少量首尾字符（或用共享 redact helper）。
- 工具入参、子进程 stdout、第三方响应体都可能含凭据——落盘或写日志前截断 / 脱敏；用户文案保持笼统（「API 密钥无效」）。
- 审 diff 时查新增的形似密钥常量，以及把 `std::env` / `process.env` 值直接拼进日志的写法。

### 4.6 安全敏感面 / Security-sensitive surfaces

动到以下任一处，必须显式检查边界，并在 PR / 回复里说明风险与缓解：

| Area | Entry points | Focus |
|------|--------------|-------|
| Workspace switching & paths | `src/web/workspace/`, `src/workspace/path.rs`, `src/workspace/fs.rs`, `src/tools/file/path.rs` | Normalization, no unauthorized arbitrary directories, `..` / symlink handling; Linux already uses `openat2` `RESOLVE_IN_ROOT` on some paths — residual TOCTOU is documented in `path_workspace` / `workspace_fs` and must not be softened. |
| Command execution | `src/tools/command.rs`, allowlist config, `run_command` in `tool_registry` | Allowlisted commands only; `..` / absolute-path argument rejections stay as-is unless intentionally changed *and* documented. |
| HTTP fetching | `src/tools/http_fetch.rs`, `http_fetch` / `http_request`, `http_fetch_allowed_prefixes` | Conservative prefixes; approval semantics for unmatched URLs (including the `http_request` METHOD key) must match the docs. |
| File read/write tools | `src/tools/file.rs` and friends | `resolve_for_read` / `resolve_for_write` semantics must not be bypassed. |

**不要**用弱化注释或文档里的安全说明掩盖未缓解的路径；已知缺口记在 `docs/待办清单.md`。

### 4.7 架构与文档同步 / Architecture & documentation sync

- 改**架构**或 `src/` 模块组织 ⇒ 更新 `docs/开发文档.md` 的「架构设计」（分层、Web 数据流、Mermaid 节点/边）与「主要模块」表；
  保持概览粒度，不要重新长出逐文件表（工具清单属 `docs/工具说明.md`）。
- 触发：`src/lib.rs` 增删 `mod`；重排 `Router` / `AppState`；新建 / 改名 / 删除 `src/` 文件或目录；某文件主职责变了；
  `tool_registry` 新增执行类别；`runtime/` / `web/` 子模块边界移动。
- 不需要：纯实现改动、私有改名、注释 / 格式、只动 `tests/` / `#[cfg(test)]` / fixtures。
- **用户可见变更**（CLI flag、API、配置键、TUI/Web 行为、工具）⇒ 同步 `README.md` + `docs/工具说明.md`；
  动 `config/*.toml`、axum 路由/handler（路径、方法、请求/响应体、新端点）或增删 `CM_*` ⇒ **必须**同步 `README.md` + `docs/开发文档.md`。
- `docs/待办清单.md`：条目实现后删除（不要留 `- [x]`）；小节空了连标题一起删。

### 4.8 Chat / SSE 契约（两端）/ Chat / SSE contract

改动须同时核对 Rust 路由/handler、Client 解析器与文档：

- **路由与请求体**：`src/lib.rs` 的 `/chat`、`/chat/stream` 及其 JSON 字段。
- **SSE 线协议**：`src/cm_sse_protocol/`（根重导出 `crate::sse`）、`SSE_PROTOCOL_VERSION`、payload 类型、错误码、`data:` 分帧。
  权威表 `docs/SSE协议.md`，对齐 Client 的 `parser_v2.rs` / `sse_dispatch/types.rs`。
- **`sse/line.rs`（`classify_agent_sse_line`）故意没有生产消费者**——它是单实现语义的锚点，**不要**当死代码清理；
  要么与 Web 消费者保持一致，要么在代码注释与 `docs/SSE协议.md` 两处写明有意分歧。
- 字段增删改名 ⇒ serde 类型 + Client `parser_v2.rs` + `sse_dispatch/types.rs` + 协议文档；控制面分发顺序或新顶层键 ⇒
  `control_classify.rs` + `fixtures/sse_ag_ui_golden.jsonl` + Client `parser_v2.rs`，并跑
  `cargo test --lib golden_ag_ui_classify_matches_expected`（及 Client 仓对应 golden）。新错误码 ⇒ 前端展示 / 重试路径，
  且响应体不得含密钥。
- **本仓没有 `frontend/` 源码**：UI 检查在 Client 仓（`make frontend-check` / `make frontend`），契约钉 `client-contract-v*`。

### 4.9 工具注册 / Tool registry (`src/cm_tools/`)

- **Args 与 spec 同源**（#1157 已迁移）：`tool_param_types/` 写 Args（`Deserialize + JsonSchema`），
  `specs/*.inc.rs` 用 `builtin_tool!` 声明一行，执行体是 `runners.rs` 的 `runner_*` 薄包装。别再手抄 `ToolSpec` 的 `parameters`。
- **`name` 必须用 `src/cm_tools/tool_names.rs` 的常量（绝不写字面量）并同时加进 `tool_names::ALL`**——
  `all_tool_names_match_specs` 会拦住漏加。同步 `README.md` + `docs/工具说明.md`。
- 零漂移：`cargo test -p crabmate --lib cm_tools::tools::tool_specs_registry`（archive 三件套需 `--features archive-tools`），
  **不带 BLESS** 跑绿才算没漂移。完整三步流程 / `ToolArgsSummary` 宽容取值 / `async_only` runner / golden 细节见
  `docs/工具说明.md`「新增 / 重命名内置工具（三步）」。

### 4.10 发布与依赖 / Release & dependencies

- 版本唯一来源是 `Cargo.toml` → `[package] version`；改版本**只走 `make set-version VERSION=X.Y.Z[-pre]`**
  （连带重解析 `Cargo.lock`、重生成 `docs/openapi.json` 与 `man/*.1`），不要手改。
- tag 格式 `vX.Y.Z`（可 `-rc.N`）；打 tag 前先把 `[Unreleased]` 收成 `## [X.Y.Z]`；push tag 即发版。
- 破坏配置键 / SSE / HTTP / 公共 CLI 属**破坏性变更**：subject 标 `!` 或 `BREAKING CHANGE:`，并写进 CHANGELOG。
- 动 `Cargo.toml` / `Cargo.lock` 时核对新 crate 许可证（`deny.toml` / `cargo deny`）、`cargo audit`、锁文件一致性。
- 全流程、CHANGELOG 该记什么、许可证细节见 `docs/发布流程.md`。

### 4.11 CLI / TUI / Web 共享逻辑 / Shared logic

D2.2 之后同进程 `chat` / `repl` / `tui` 已硬删。现存面：本仓运维 CLI（`serve`、`doctor`、`web-bearer`、`save-session`、
  `tool-replay`）、Client **`crabmate-tui`**（HTTP + SSE）、Client Web / Desktop（本仓 `lib`）。

- 每加一个能力先判断是否**天生只属于一个面**；**不是**就让 ops CLI / `crabmate-tui` / Web 都能到达（差异只在交互与呈现）。
- **是**天生的可以只做一个面，但**数据模型、工具执行与配置键仍属共享层**（`agent/`、契约类型），以便其余面接入。
- 与 UI 无关的规则放可复用模块（`agent/`、`types`、`sse::protocol`、`tool_registry`、`runtime/` 非 UI 部分）；
  session / export 形状已在 `runtime/workspace_session`、`runtime/chat_export`，扩展同一套契约。

## 5. Git 与 GitHub 协作 / Git & GitHub flow

- 默认 **GitHub Flow**：短分支 → PR → 检查通过且评审后回 `main`；`main` 始终可发布，不做 feature 直接 push。
- **先有 Issue**：新计划、设计、非平凡重构必须先有跟踪 Issue（问题 / 范围 / 明确非目标 / `docs/` 锚点）；typo、一行 bug 可跳过。
  草稿留 `agent_space/`，Issue 定稿后毕业进 `docs/design/`。
- **分支命名** `type/short-slug`（有 Issue 就带编号，如 `feat/842-ltm-hybrid-retrieval`）；`main` 是唯一长期分支，合并后删分支。
- **PR ↔ Issue**：正文写 `Closes #123`（本 PR 完成）或 `Refs #123`（只推进，Issue 保持 open）；
  不要在 PR 里悄悄扩大范围，另开 follow-up Issue。
- 开 / 合 PR 前至少 `pre-commit run --all-files` 且 CI 绿；push 前先 rebase 到最新 `origin/main`。
- **Stacked PR 的 `CONFLICTING` 处理、`--force-with-lease=<ref>:<sha>` 显式写法、冲突复现步骤见 `docs/git协作流程.md`**。

## 6. 文档索引 / Where to look

| 想知道 | 去哪 |
| --- | --- |
| 配置项 / 供应商 / 认证模式 / 热重载 | `docs/配置说明.md` |
| 子命令 / HTTP 路由 / 日志级别 / 打包 | `docs/命令行与路由.md` |
| 工具清单 / 权限等级 / 新增工具三步 | `docs/工具说明.md` |
| 架构 / 主要模块 / 扩展点 | `docs/开发文档.md` |
| SSE 行协议 / 错误码 | `docs/SSE协议.md` |
| 环境陷阱 / 缺失 CLI / 探针 / 前端构建 | `docs/调试指南.md` |
| 测试 / CI 对齐 / E2E | `docs/测试指南.md` |
| 发版 / CHANGELOG / 依赖许可 | `docs/发布流程.md` |
| 分支 / 提交 / Stacked PR 细则 | `docs/git协作流程.md` |
