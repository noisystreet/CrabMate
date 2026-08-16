# ADR：单包 `crabmate` 发布到 crates.io

> **状态**：**Accepted**（2026-08-16）  
> **入口**：PR 853 / `client-contract-v0.2.0` 已合入 `main`（W2b：本仓无 `crabmate-tool-card`）。S2 **#854**、公开面收口 **#855**（`27c1fd3a`）已合 `main`。Client **S3** 钉该 rev + `protocol`。S4.5 **#857** 已合 `main`。  
> **对齐**：[`client_shell_split.md`](./client_shell_split.md) 路径 A；[`client_contract_versioning.md`](./client_contract_versioning.md)；展示下沉 [`client_display_crate_sink.md`](./client_display_crate_sink.md)（W3/W4 **缓做**，不阻塞本计划）。  
> **Client 消费**：合入后钉 **一个** crate `crabmate`，`default-features = false, features = ["protocol"]`；禁止再 git 钉 `crabmate-sse-protocol` 等旧包名（`v0.3.0` / `client-contract-v0.2.0` 旧 Client 仍可用）。

---

## 1. Context

切仓前本仓是 Cargo workspace（根包 `crabmate` `0.3.0` + ~19 个 `crates/*`）。`cargo publish -p crabmate` 会把 path 依赖换成 crates.io 上的**同名版本**：内部 crate 只要还是独立 package，就会出现在 registry 上。S2 后树内只有根包 **`crabmate`**。

目标：**crates.io 上只有 `crabmate` 一个包**，同时官方 Client（WASM `frontend` + native `crabmate-tui-core`）仍能消费线契约，且**不能**链接 `tokio` 运行时 / `nix` / `rusqlite` / `axum`。

约束：

1. 已发布 crate 不得 path 依赖 `publish = false` 的 workspace 成员。
2. 把现有成员**逐个**并进根包不可行：叶子（如 `types`）并入根包后，`sse-protocol` 等仍依赖 `types`，若改依赖根包会与「根包依赖 sse-protocol」成环。**切仓必须一次完成。**
3. Client **禁止** `path` 回本开发树（`check-no-main-path.sh`）。切仓后钉 git tag / crates.io `version`，带 `protocol` feature。
4. 线协议字节与 HTTP JSON **不**随本次搬家而变；破坏的是 **Cargo 包名**（`crabmate-sse-protocol` → `crabmate::cm_sse_protocol`）。
5. 产品 git tag **`v0.3.0` 不得**用作 crates.io `0.3.0`：树形状不同。首发单包用 **`0.4.0`**。

---

## 2. Decision

| 项 | 选择 |
|----|------|
| crates.io 包 | 仅 **`crabmate`** |
| 首发版本 | **`0.4.0`**（相对 git `v0.3.0` 为 Cargo 包图 BREAKING） |
| 默认 feature | **`server`**（`cargo install crabmate` / `serve` / 运维 CLI） |
| Client feature | **`protocol`**：类型、SSE 分类/帧、OpenAPI DTO、display-rules、turn-layout、chat-export；**无** `rt-multi-thread` / `net` / `process`、`nix`、`rusqlite`、`axum` |
| `turn-layout` | **留在本仓**，作为 `protocol` 模块；**不做** display-sink W3/W4 |
| `tool-card` | 已在 Client；不进本包 |
| 旧 git 钉 | `v0.3.0` / `client-contract-v0.2.0` **永久可取**；新 Client 不跟 |

### 2.1 Feature 切分

```toml
[features]
default = ["server"]
server = ["dep:tokio", "dep:axum", "dep:rusqlite", /* 现有 web/mcp/… 门控 */]
protocol = []
# 现有：mcp / gen-man / docker_sandbox / fastembed / project_metrics —— 仅 server 侧，不得被 protocol 打开
```

第三方依赖凡仅 server 使用的，一律 `optional = true`，由 `server`（或更细 feature）启用。`nix` 保持 `[target.'cfg(unix)'.dependencies]`。

`protocol` 允许的第三方（白名单，S1 冻结）：`serde` / `serde_json` / `thiserror` / `schemars` / `log`（若分类路径需要）。**禁止**进入 `protocol` 编译图：`tokio`（含仅 `sync`）、`reqwest`、`axum`、`rusqlite`、`nix`、`tiktoken-rs`、`worbrow`。

> 现状：Client 已 git 钉带 `tokio` `sync` 的 `crabmate-sse-protocol`，wasm 能过。单包后**仍禁止**把 `tokio` 放进 `protocol`，避免根包再导出 `stream_hub` 时误开 runtime。

### 2.2 合并后模块名（Client `use` 映射）

规则：`crabmate-<x>` → 模块 **`cm_<x>`**（连字符改下划线）。crates.io 包名仍是 **`crabmate`**。

| 旧 package | 新路径（`features = ["protocol"]`） |
|------------|-------------------------------------|
| `crabmate-types` | `crabmate::cm_types` |
| `crabmate-display-rules` | `crabmate::cm_display_rules` |
| `crabmate-api-contract` | `crabmate::cm_api_contract` |
| `crabmate-chat-export` | `crabmate::cm_chat_export` |
| `crabmate-turn-layout` | `crabmate::cm_turn_layout` |
| `crabmate-sse-protocol` | `crabmate::cm_sse_protocol`（**不含** `stream_hub` / `mpsc_send` / 审批桥） |

根包可再导出别名（**仅 `server` 组合面**，减少 `src/` 改写）：`cm_types` → `types`，`cm_sse_protocol::sse` → `sse`，`cm_config` → `config`。这些别名 **`cfg(feature = "server")`**，`protocol`-only **编译不过** `crabmate::types` / `crabmate::sse`。Client **不要**依赖这些别名，只钉 `cm_*`。

Client 示例：

```toml
crabmate = { git = "https://github.com/noisystreet/CrabMate", tag = "v0.4.0", package = "crabmate", default-features = false, features = ["protocol"] }
# 上 crates.io 后改为：
# crabmate = { version = "0.4.0", default-features = false, features = ["protocol"] }
```

```rust
use crabmate::cm_sse_protocol::{classify_ag_ui_sse_data, SSE_PROTOCOL_VERSION, StreamEndReason};
use crabmate::cm_api_contract::StatusShellView;
use crabmate::cm_turn_layout::project_turn_web_v2;
```

`crabmate-tui-core` 自身已有 `tokio`，**仍然只开 `protocol`**，不要为图省事开 `server`。

### 2.3 SSE：`protocol` vs `server`

今日 `crates/crabmate-sse-protocol` 混了两层：

| 进 `protocol` | 仅 `server` |
|---------------|-------------|
| `SSE_PROTOCOL_VERSION`、`classify_*`、`sse_frame`、`StreamEndReason` | `sse::stream_hub`、`mpsc_send`、`control_mirror`、`web_approval`、终态 `send_*`（`Sender<String>`） |
| `sse::protocol` 载荷类型与纯函数 `encode_message` | 依赖 `tokio::sync::mpsc` / `broadcast` 的桥 |

S1 先在**现 workspace** 把 `tokio` 改为 `runtime` feature（见 §5），切仓时同一边界收在 `crabmate::cm_sse_protocol` 内用 `cfg(feature = "server")`（不再单列 `sse_runtime` 模块）。

### 2.4 首发 `0.4.0` 的 semver 承诺

`0.4.0` 把默认 `server` 面冻进 crates.io。**承诺**与 **不承诺** 必须在改 `version` 之前写进 rustdoc / 中英 README（波次 **S4.5**），避免下游把 `#[doc(hidden)]` 当成稳定 SDK。

| 承诺（semver） | 不承诺 |
|----------------|--------|
| **`protocol`**：六个模块 `cm_types` / `cm_api_contract` / `cm_sse_protocol` / `cm_turn_layout` / `cm_display_rules` / `cm_chat_export` 上 Client 已用的符号 | `#[doc(hidden)]` 的 `cm_agent` / `cm_llm` / `cm_config` / `cm_workflow` / `cm_internal`、`e2e_scenario`、`test_serve` |
| **`server`**：组合面模块名 `agent` / `config` / `llm` / `sse` / `types` 的**存在**；根上显式 `pub use`（`run`、`run_agent_turn`、`build_tools*`、`ProcessHandles`、`tool_sandbox` 等，以 `src/lib.rs` 为准） | `agent::agent_turn` 等组合面**内部路径**；把 `crabmate` 当通用嵌入式 Agent SDK |
| 线协议：`docs/SSE协议.md` + `cm_api_contract` 错误码 + `ChatRequestBody` | 静态 SPA / `/uploads` 文件与 `CM_E2E_FIXTURES` 路由不进 OpenAPI |

HTTP 对官方 Client 已够用（`/chat`、`/chat/stream`、`/chat/async`、审批、会话 revision）。默认 `web_api_require_bearer=false` 与「密钥为空则中间件不校验」是产品选择，**不**在首发前改行为。

**首发前不做**（会把 S4 拖成又一次大切仓；若将来要做须**单独 PR 且在 S5 之前**，否则 `0.4.0` 后再缩是又一次 breaking）：

- 把 hidden 模块改成真正 `pub(crate)`（E0365：整模块 `pub use` 需要源模块保持 `pub`）
- 把 `server` 拆成无 axum 的 agent + `web` + `mcp`
- 合并 `src/agent` 与 `src/cm_agent` 两棵 `agent_turn`
- 给 `/chat` 加幂等键；收紧默认 Bearer

---

## 3. Consequences

**好处**

- `cargo install crabmate` / `cargo add crabmate` 只对一个包。
- 内部实现不出现在 crates.io 包列表。
- Client 钉点从 5～6 条 git package 收到 1 条。
- 改气泡投影不必再为「多 crate 版本对齐」发一串包（仍建议打 git tag 供 Client 在 publish 前联调）。

**代价**

- 一次大切仓：各原 crate 内 `crate::` 改为 `crate::<mod>::`（或等价 `super`）。
- 禁边脚本 `check-crate-deps.sh`（`cargo tree -p`）失效，须改成**模块** DAG 检查。
- Client 全量改 `use` + lockfile；旧包名无法从同一 crate 再导出为第二个 crates.io 名。
- 根包 `lib.rs`：**`protocol` 为稳定契约**（六个 `cm_*`）；**`server` 组合面** `agent` / `config` / `llm` / `sse` / `types` 与显式 `pub use`。无再导出的实现模块 `pub(crate)`；因整模块 `pub use` 必须保持 `pub` 的标 `#[doc(hidden)]`。

**后续约束**

- 线协议仍以 `docs/SSE协议.md` + `fixtures/sse_*_golden.jsonl` 为权威。
- `docs/Turn布局设计.md` 投影实现指针改为 `crabmate::cm_turn_layout`（本仓模块），不迁 Client。
- 新的仅-WASM 依赖不得加入 `protocol` feature 的依赖边。
- 首发前按 **§2.4 / S4.5** 写清白名单；`doc(hidden)` **不是** semver 围栏。

---

## 4. Alternatives Considered

| 方案 | 否决原因 |
|------|----------|
| 维持 workspace，内部 crate 也 publish | 与「一个包」目标相反 |
| 内部 `publish = false`，只发根包 | Cargo 不允许已发布包依赖未发布 path 成员 |
| 只发契约 6 包 + git 安装二进制 | 不是单包；使用者仍要记一串 crate |
| 并进根包且 Client 依赖默认 feature | WASM 链 `tokio` runtime / `rusqlite` 等 |
| 先 W3 把 `turn-layout` 迁 Client 再单包 | 不减小 publish 图；切仓时多一次双仓搬家 |
| 兼容空壳包 `crabmate-sse-protocol` 转发到 `crabmate` | 又变多包 |
| 第三仓只放 protocol | 三仓发版税；与「一个包」相反 |

---

## 5. 波次

每波可单独 PR。S2 **必须**一整条分支切完再合 `main`（禁止合到一半的「半 workspace」）。

```text
S0  本文 + 索引（本 PR）
S1  现仓：sse-protocol 切开 protocol/runtime（tokio 可选）
S2  切仓：单一 [package]，feature 门控，禁边脚本改模块 DAG
S3  Client：钉 v0.4.0-pre / rev，features=["protocol"]
S4.5 稳定面说明 + OpenAPI 补洞（**先于**改 version）
S4  元数据 + cargo publish --dry-run
S5  crates.io 真发 0.4.0 + 文档 / Client 改 version
```

S3 可与 S2 末尾叠：S2 合入并打 **`v0.4.0-alpha.1`**（或 `rev`）后 Client 再合。禁止 S5 早于 S3 绿。**S4.5 须在 S4.1 改 `version` 之前合入**（可与 S4 同波，但不要和 dry-run 塞进同一个 PR）。

### S0 — 文档

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| S0.1 | Server | 本文；`client_display_crate_sink` 标明 W3 缓做；待办 / 开发文档 / versioning 入口 | 本 PR |
| S0.2 | Client | `display_crate_sink.md` / `contract_pin.md`：W3 缓做；单包钉法指向本文 | 与 S0.1 同期或紧随 |

### S1 — `sse-protocol` feature（仍为独立 crate）

**入口**：S0。**目的**：切仓前证明「无 tokio 的协议面」测试与 wasm 都绿。

| ID | 动作 | 验收 |
|----|------|------|
| S1.1 | `crabmate-sse-protocol`：`default = ["runtime"]`，`runtime = ["dep:tokio"]`；hub/mpsc/审批桥 `#[cfg(feature = "runtime")]` | `cargo test -p crabmate-sse-protocol --no-default-features` 覆盖 classify / 金样 |
| S1.2 | 根包 `server` 路径启用 `crabmate-sse-protocol/runtime` | `cargo test -p crabmate` 现有 SSE 测仍绿 |
| S1.3 | 增加 wasm 冒烟：临时或 `examples/` 仅依赖 `--no-default-features` 的 sse-protocol | `cargo check --target wasm32-unknown-unknown`（无 `rt`/`net`） |
| S1.4 | 更新 `check-client-contract.sh` 消费者：可 `default-features = false` | 脚本绿 |

Client **此波不必改钉**（仍用默认 feature 的 git tag 也能编）。

### S1 落地（2026-08-16）

- **S1.1**：`default = ["runtime"]`，`runtime = ["dep:tokio"]`；`stream_hub` / `mpsc_send` / `web_approval` / `control_mirror` / `final_response_terminal` 均 `#[cfg(feature = "runtime")]`。
- **S1.2**：根包、`crabmate-internal`、`crabmate-approval`、`crabmate-llm` 显式 `features = ["runtime"]`。
- **S1.3 / S1.4**：`scripts/check-sse-protocol.sh` 跑 `--no-default-features` 测、`cargo tree` 禁 tokio、wasm32 `--lib` check（无该 target 则 skip）；`check-client-contract.sh` 消费者 `default-features = false`。

### S2 — 切仓（一次 PR / 一条长分支）

**入口**：S1 绿。工作区最终：

```toml
[workspace]
members = ["."]
resolver = "2"
```

（若工具脚本假设 `crates/*` 成员，同步改 pre-commit / lizard / CI cache paths。）

### S2.1 冻结：包名与模块目录（2026-08-16）

**包名**：crates.io / Cargo 只有 **`crabmate`**。不再保留 `crabmate-sse-protocol` 等 package 名（兼容空壳包否决，见 §4）。

**规则**：`crabmate-<x>` → 目录 / `mod` **`cm_<x>`**（`-` → `_`）。与现有 `src/agent`、`src/llm`、`src/runtime` **自然错开**，不必再发明 `agent_domain` 一类名字。

**原则**

1. 现有根包 `src/{agent,llm,runtime,web,…}` **保持 composition**（`crate::agent` 执行面路径不变）。
2. 原 workspace 成员 **1:1** 变成顶层 `mod cm_*`，目录 `src/cm_*/`（由 `crates/crabmate-*/src` `git mv`）。
3. 不把两棵 `agent_turn` 树硬并成一个目录（`src/agent/agent_turn` vs `src/cm_agent/agent_turn`）。
4. `protocol` feature 只编译下表「P」列；其余 `cfg(feature = "server")`（默认）。
5. 原 crate 内 `crate::foo` → `crate::cm_<x>::foo`。根包 `src/` 里 `crabmate_<pkg>::` → `crate::cm_<x>::`。
6. 组合面别名（`types` / `sse` / `config` / `crate::tools`）仅服务本仓 `src/`，**`cfg(feature = "server")`**，不作为 Client 契约。
7. 无组合面模块再导出的 server 实现（`cm_tools`、`cmd_mate`、`cm_runtime`、`cm_memory` 等）为 **`pub(crate)`**。`cm_agent` / `cm_config` / `cm_llm` / `cm_workflow` / `cm_internal` 因 `pub use` 整模块（`agent` / `config` / `http_client` / `workflow` / `tool_sandbox`）须保持 `pub`，标 **`#[doc(hidden)]`**，不作为 Client 契约。

**对外（Client / `protocol`）** — 与 §2.2 一致：

| 旧 package | 目录 / `mod` | feature |
|------------|----------------|---------|
| `crabmate-types` | `src/cm_types/` | P |
| `crabmate-display-rules` | `src/cm_display_rules/` | P |
| `crabmate-api-contract` | `src/cm_api_contract/` | P |
| `crabmate-chat-export` | `src/cm_chat_export/` | P |
| `crabmate-turn-layout` | `src/cm_turn_layout/` | P |
| `crabmate-sse-protocol` | `src/cm_sse_protocol/` | P（无 tokio）；hub/mpsc 等 `cfg(server)` |

根包：`pub use crate::cm_sse_protocol::sse` **仅 server**，保持本仓 `crate::sse::…`。`protocol`-only 只用 `crabmate::cm_sse_protocol`。

**对内（仅 `server`）**

| 旧 package | 目录 / `mod` | 说明 |
|------------|----------------|------|
| （已有） | `src/agent/` `src/llm/` `src/runtime/` | **不动** |
| `crabmate-agent` | `src/cm_agent/` | 今日 `src/agent/mod.rs` `pub use crabmate_agent::…` |
| `crabmate-llm` | `src/cm_llm/` | 今日 `pub use crabmate_llm` |
| `crabmate-runtime` | `src/cm_runtime/` | 退出码、消息展示、`save-session` 辅助 |
| `crabmate-config` | `src/cm_config/` | 可 `pub use cm_config as config` |
| `crabmate-tools` | `src/cm_tools/` | 经 `cm_internal` 再导出可继续叫 `crate::tools` |
| `crabmate-memory` | `src/cm_memory/` | |
| `crabmate-workflow` | `src/cm_workflow/` | `crate::agent::workflow` 再导出可保留 |
| `crabmate-approval` | `src/cm_approval/` | |
| `crabmate-web-host` | `src/cm_web_host/` | 不与 `src/web/` 合并 |
| `crabmate-mcp` | `src/cm_mcp/` | |
| `crabmate-benchmark` | `src/cm_benchmark/` | 不与 `src/runtime/benchmark/` 合并 |
| `crabmate-internal` | `src/cm_internal/` | **整包一座**，S2 不摊平 |
| `cmd_mate` | `src/cmd_mate/` | **不是** `crabmate-*`，模块名保持 `cmd_mate` |

**不要在 S2 做**：把 `cm_internal` 拆进根；把两棵 `agent_turn` 合成一目录；给 Client 再导出第二个 crates.io 包名。

物理结果（示意）：

```text
src/
  lib.rs
  cm_types/ cm_display_rules/ cm_api_contract/ cm_chat_export/ cm_turn_layout/ cm_sse_protocol/
  cm_agent/ cm_llm/ cm_runtime/ cm_config/ cm_tools/ cm_memory/ cm_workflow/
  cm_approval/ cm_web_host/ cm_mcp/ cm_benchmark/ cm_internal/
  agent/ llm/ runtime/ web/ chat_job_queue/ …   # 原根包 composition
  cmd_mate/
```

| ID | 动作 | 验收 |
|----|------|------|
| S2.1 | ~~冻结模块表~~ **已写入本节** | 切仓按上表，不现场改名 |
| S2.2 | ~~`git mv` + 改写 `crate::`；单 `[package]`~~ **已落地** | `cargo check --features server` |
| S2.3 | ~~`protocol` wasm / `cargo tree` 无 tokio/nix/rusqlite/axum~~ **已落地** | `--no-default-features --features protocol --target wasm32-unknown-unknown --lib` |
| S2.4 | ~~`check-crate-deps.sh` 改为模块 DAG~~ **已落地** | `src/cm_workflow` ↛ `crate::cm_internal` 等 |
| S2.5 | ~~`check-client-contract.sh` 只钉 `crabmate` + `protocol`~~ **已落地** | 外仓风格消费者脚本 |
| S2.6 | ~~金样 `CARGO_MANIFEST_DIR` 改根~~ **已落地** | `fixtures/` 与 `src/cm_*/fixtures/` |
| S2.7 | ~~文档模块表~~ **已落地** | `开发文档` / `Turn布局设计.md` / `crate_dep_policy.md` |

**禁止**：S2 合入后仍留 `crates/crabmate-*` 作为第二套源码。

### S3 — Client 钉单包

**入口**：S2 / #855 已在 `main`（钉点 `27c1fd3a`；或日后 tag `v0.4.0-alpha.N`）。

| ID | 仓 | 动作 | 验收 |
|----|------|------|------|
| S3.1 | frontend | 一条 `crabmate` 依赖；删除其余 Server git package；改 `use` | `cd frontend && cargo test --lib`；wasm clippy |
| S3.2 | tui-core | 同上，仅 `protocol` | `cargo test`（该 crate 目录） |
| S3.3 | `contract_pin.md` / `check-no-main-path.sh` | 允许 git/crates.io `crabmate`，仍禁 path 回 agent 仓 | 脚本绿 |
| S3.4 | Playwright / `make frontend-check` | 与 Server `v0.4.0-alpha` 或 `rev` 对齐 | CI 绿 |

### S4.5 — 契约说明与 OpenAPI 补洞（先于改 version）

**入口**：S3 绿（Client 已钉 `protocol`）。**单独 PR**，不要和 S4.1–S4.3 绑在一起。

| ID | 仓 | 动作 | 验收 |
|----|------|------|------|
| S4.5.1 | rustdoc `src/lib.rs` + 中英 README | 写清 §2.4 白名单：`protocol` 六模块；`server` 显式入口；hidden / 内部路径不承诺 | 读者能区分「线契约」与「带库的服务器」 |
| S4.5.2 | 将打进 `cargo package` 的注释 | 去掉仍写旧 workspace 包名的误导（如 `cm_api_contract` 仍称 `crabmate-web-host`） | 发布包内无「本 crate crabmate-*」类过时指称 |
| S4.5.3 | `GET /openapi.json` | 补已挂载的 `/user-data/mcp-servers*`、`PUT /user-data/secrets/web-api-bearer`；测试从 `src/web/routes/**`、`server.rs`、`cm_web_host` 的 `.route(` 收集 **path+method** 对照 OpenAPI（排除 `e2e_fixtures` / 静态） | 漏路径或漏 method 会 fail；OpenAPI 仍**不**替代 `docs/SSE协议.md` |

### S4 — publish 准备

**入口**：S3 + S4.5 绿。

| ID | 动作 | 验收 |
|----|------|------|
| S4.1 | `[package]`：`version = "0.4.0"`、`repository`、`readme`、`include`（`config/`、`man/`、`LICENSE*`；排除巨大无关夹具若有） | `cargo package --list` 合理 |
| S4.2 | `cargo publish --dry-run --allow-dirty` 在干净树改为不 dirty | dry-run 成功 |
| S4.3 | `cargo deny` / 许可证与 `deny.toml` | 与 CI 一致 |
| S4.4 | README：`cargo install crabmate`；Client 钉 `0.4.0` + `protocol`（可与 S4.5.1 同改 README，但 **version 数字仍放本步**） | 中英 README 各一段 |

### S5 — 真发

**入口**：S4 dry-run 绿（含 S4.5）。

| ID | 动作 | 验收 |
|----|------|------|
| S5.1 | crates.io `cargo publish`（需维护者 token；**不进仓库**） | `https://crates.io/crates/crabmate` 显示 0.4.0 |
| S5.2 | git 注释标签 **`v0.4.0`**（产品 + 单包同源） | tag 指向 publish 的 commit |
| S5.3 | Client 改 `version = "0.4.0"`（可去掉 git） | frontend / tui-core CI 绿 |
| S5.4 | `client_contract_versioning.md`：默认渠道改为 crates.io **或** git `v0.4.0` 二选一写清 | 与 `contract_pin.md` 一致 |

---

## 6. 验证矩阵

| 检查 | 命令 / 位置 |
|------|-------------|
| protocol / wasm | `cargo check -p crabmate --no-default-features --features protocol --target wasm32-unknown-unknown`（S2 后无 `-p` 即根包） |
| protocol 依赖图 | `cargo tree --no-default-features --features protocol` 不含 tokio/nix/rusqlite/axum |
| server | `cargo test`（default / `--features server`） |
| 契约门禁 | `bash scripts/check-client-contract.sh` |
| OpenAPI vs 路由 | S4.5.3 落地后的路由表对照测试 |
| Client | `make frontend-check`；tui-core 测试 |
| 禁 path | Client `scripts/check-no-main-path.sh` |
| publish | `cargo publish --dry-run`（S4；**S4.5 已合**） |

---

## 7. 与展示下沉、契约 tag 的关系

| 项 | 本计划 |
|----|--------|
| W1 / W2b | **已完成**；`tool-card` 不回本仓 |
| W3 / W4 / W5 | **缓做**（无期限）；单包后 `turn-layout` 已是本仓模块，再迁 Client 无益于 crates.io |
| `client-contract-v0.2.0` | 旧多包钉点；S5 后新 Client 不再需要 `client-contract-v*` 多 package |
| B2（布局元数据） | 金样继续留本仓 `cm_turn_layout` 模块；不把「迁 Client」当 B2 前置 |

---

## 8. 回滚

- **S1**：还原 feature；Client 未改钉，回滚面小。
- **S2 合入前**：丢弃切仓分支。
- **S2 已合、S5 未发**：恢复 workspace 需 git revert 整波；旧 Client 仍钉 `v0.3.0`。
- **S5 已 publish**：crates.io 版本不可删；只能发 `0.4.1` / `0.5.0`。yank `0.4.0` 仅防新下载，不从 registry 抹掉。

---

## 修订记录

| 日期 | 说明 |
|------|------|
| 2026-08-16 | 初稿：单包 `0.4.0` + `server`/`protocol`；切仓一次完成；W3 不阻塞 |
| 2026-08-16 | 收紧顶层 `pub`：protocol 无别名；server `cm_*` 实现模块 `pub(crate)` |
| 2026-08-16 | S4.5：首发前写清 semver 白名单与 OpenAPI 漏路径；可见性再收 / feature 拆分不进 version PR |
| 2026-08-16 | S4：`version = "0.4.0"`、`include`、README `cargo install crabmate`；S5 再 publish / 打 tag |
