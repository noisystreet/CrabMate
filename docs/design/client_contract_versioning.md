# Client 契约发版与钉版本（Phase 1）

> **状态**：采纳（2026-08-08）— 支撑 [`client_shell_split.md`](./client_shell_split.md) 路径 A。  
> **执行勾选**：[`client_shell_split_todo.md`](./client_shell_split_todo.md) Phase 1。  
> **人读协议**：[`docs/SSE协议.md`](../SSE协议.md)、[`docs/命令行契约.md`](../命令行契约.md)（HTTP `ApiError` / OpenAPI）。  
> **门禁脚本**：`scripts/check-client-contract.sh`（SSE 金样 + OpenAPI 冒烟 + 外仓风格 path 消费）。

---

## 1. 目标

壳仓 / 独立 UI / 浏览器静态包 **不经 monorepo `path` 回主 workspace**，即可钉住：

| 面 | Crate / 产物 | 用途 |
|----|----------------|------|
| HTTP JSON DTO + 错误码 + OpenAPI schemars | `crabmate-api-contract`（及传递依赖 `crabmate-types`、`crabmate-display-rules`） | 请求/响应形状、`ApiError.code` |
| SSE 控制面 | `crabmate-sse-protocol` | `SSE_PROTOCOL_VERSION`、载荷类型、分类 |
| Tauri 连接页逻辑 | `crabmate-connect` | 探测 `/health`、Bearer hash、钥匙串（**非**主 workspace 成员） |

**默认发版渠道**：**本仓 git 注释标签**（见 §4）。**暂不**要求 `cargo publish` 到 crates.io；待供应链与版本节奏稳定后再开（可选）。

---

## 2. 两套版本轴（勿混用）

### 2.1 线协议：`SSE_PROTOCOL_VERSION`（`u8`）

- 常量：`crabmate_sse_protocol::SSE_PROTOCOL_VERSION`（当前 **`2`**）。
- 出现在：控制面 JSON 顶层 **`v`**、首帧 **`sse_capabilities.supported_sse_v`**、请求可选 **`client_sse_protocol`**。
- **Bump 条件**：控制面 JSON **形状或语义**对旧客户端不兼容（删/改顶层键含义、改变必选字段、改变官方前端必须识别的分类结果等）。
- **不 bump**：软字段（旧客户端忽略即可），例如可选 **`request_id`** / AG-UI **`requestId`**、`sse_capabilities.terminal_order` 等（见 `docs/SSE协议.md`）。

协商与错误码（保持可预期，勿擅自改码名）：

| 场景 | 码 |
|------|-----|
| 请求 `client_sse_protocol == 0` | `INVALID_SSE_CLIENT_PROTOCOL` |
| 请求 `client_sse_protocol >` 服务端版本 | `SSE_CLIENT_TOO_NEW` |
| 请求正整数且 `<` 服务端版本 | `SSE_PROTOCOL_MISMATCH` |
| 首帧 `supported_sse_v` ≠ 官方客户端本地常量 | 客户端侧文案含 `SSE_SERVER_TOO_NEW` / `SSE_SERVER_TOO_OLD` |

### 2.2 Cargo crate semver（`Cargo.toml` `version`）

对 **Rust API**（类型增删、函数签名、错误码常量重命名等）遵循 [Cargo semver](https://doc.rust-lang.org/cargo/reference/semver.html)：

| 变更 | 建议 |
|------|------|
| 破坏性 Rust API 或破坏性 HTTP/OpenAPI 字段 | **major**（`0.y.z` 阶段可用 **minor** 表示破坏，但须在发版说明写清） |
| 向后兼容新增 DTO 字段 / 新错误码常量 / 新软 SSE 字段类型 | **minor**（或 `0.y` 的 patch+说明） |
| 文档、测试、非公开实现 | **patch** |

**线协议 bump 时**：至少 bump `crabmate-sse-protocol` 的 crate 版本，并同步中英 `docs/SSE协议.md`、金样与 `scripts/check-sse-protocol.sh` 所覆盖测试。

`crabmate-api-contract` 与 OpenAPI / `docs/命令行契约.md` 中的 HTTP 码表同发布节奏；破坏性 HTTP JSON 变更须在发版说明与 OpenAPI 中标明。

---

## 3. 兼容窗口（N / N-1）

### 3.1 线协议（当前）

**官方 Client 在收到 `sse_capabilities` 后要求与本地 `SSE_PROTOCOL_VERSION` 完全一致**——**尚无**多版本解码器窗口。

- 省略 `client_sse_protocol`：服务端不因该字段拒绝（兼容未声明的旧调用方）。
- **显式**声明且低于服务端：HTTP **`SSE_PROTOCOL_MISMATCH`**（不是静默降级）。
- 软字段：新服务端 + 旧客户端在**同一** `SSE_PROTOCOL_VERSION` 下应可忽略新键继续工作。

因此：**线协议的「N−1」窗口 = 无**（除「未声明 client 版本」与软字段）。发 bump 前须协调 Client 发版；错位时依赖上表错误码，勿改成模糊 500。

### 3.2 Crate 依赖（壳仓）

壳 / UI 仓可钉：

- **精确 tag**（推荐生产），或
- 同一 major 下的 **N 与 N−1 minor**（仅当 semver 承诺兼容且线协议版本仍匹配）。

运行时仍以线协议常量为准：crate 旧、线协议新 → 仍应按 §2.1 失败并带稳定码。

### 3.3 HTTP OpenAPI

- 机器可读：`GET /openapi.json`（`src/web/openapi` + `crabmate-api-contract` schemars）。
- 破坏性路径/schema：bump `crabmate-api-contract`，更新文档；旧 Client 对未知字段应忽略（现有可选字段惯例）。

---

## 4. Git 标签与外仓钉法（默认）

### 4.1 标签命名

在本仓（`https://github.com/noisystreet/CrabMate`）打注释标签：

```text
client-contract-vX.Y.Z
```

含义：该提交上，下列 crate 的 **`Cargo.toml` version** 与文档/金样一致，可供外仓钉住：

- `crabmate-api-contract`
- `crabmate-sse-protocol`
- 其 workspace 传递依赖（至少 `crabmate-types`、`crabmate-display-rules`）
- `crabmate-connect`（同提交；见 §5）

发标签前本地/CI 须绿：`bash scripts/check-client-contract.sh`。

首枚标签可在首次外仓消费前打；在此之前外仓可用 **`rev = "<commit sha>"`** 钉同一形状。

### 4.2 主 workspace 契约（`api-contract` / `sse-protocol`）

外仓 **Cargo.toml** 示例（**禁止**再 `path = "../CrabMate/crates/..."` 回主开发树）：

```toml
[dependencies]
crabmate-api-contract = { git = "https://github.com/noisystreet/CrabMate", tag = "client-contract-v0.1.0", package = "crabmate-api-contract" }
crabmate-sse-protocol = { git = "https://github.com/noisystreet/CrabMate", tag = "client-contract-v0.1.0", package = "crabmate-sse-protocol" }
```

Cargo 会拉取该 tag 的 workspace，并解析成员的 `workspace = true` 依赖。

开发期也可用 `rev = "<sha>"` 代替 `tag`。

### 4.3 与 crates.io（可选未来）

若改为 publish：同一 `X.Y.Z` 发布上述 crate，外仓改为 `version = "X.Y.Z"`；标签策略可保留作镜像。**当前默认仍是 git tag。**

---

## 5. `crabmate-connect`（壳专用）

| 项 | 约定 |
|----|------|
| 位置（路径 A） | **仅**在 Client 仓 `../crabmate-client/crates/crabmate-connect/`（Phase 4.1 起主仓已移除） |
| 主仓历史 | 曾位于本仓 `crates/crabmate-connect/`（非根 workspace members；`exclude` + 空 `[workspace]`）；曾可用 `git`+`path` 钉主仓 tag——**已废弃** |
| `publish` | `publish = false`（默认走 Client 仓 path / git，不进 crates.io） |
| Tauri | **`tauri = "2"`**（与 Client 仓 `desktop-tauri` / `mobile-tauri` 一致；升级 major 须壳仓同步） |
| 钉法 | Client 仓内：`crabmate-connect = { path = "../../crates/crabmate-connect" }` |

兼容说明：connect 只做探测与 Bearer 交接，**不** embed `SSE_PROTOCOL_VERSION`；协议错位仍由 UI/`serve` 按 §2.1 报错。

壳仓禁止 path 回主开发树的检查：Client 仓 `scripts/check-no-main-path.sh`。

---

## 6. CI 与本地门禁

| 检查 | 命令 / 位置 |
|------|-------------|
| 汇总（Phase 1 验收） | `bash scripts/check-client-contract.sh` |
| SSE 金样子集 | `bash scripts/check-sse-protocol.sh` |
| OpenAPI schemars + 核心 paths | 脚本内 `cargo test -p crabmate-api-contract` / `openapi_spec_has_core_paths_and_version` |
| 外仓风格消费 | 脚本内临时 crate：仅 **path** 依赖契约 crate（不加入本 workspace members） |
| 主 CI | `.github/workflows/ci.yml` job **`client-contract`** |

工作区全量 `cargo test` 仍覆盖更多金样；上表为契约发版的**显式**门禁。

---

## 7. 维护者清单（bump 时）

1. 改代码与金样（`fixtures/sse_*_golden.jsonl` 等）。
2. 同步中英 `docs/SSE协议.md` / `SSE_PROTOCOL.md`；HTTP 码表则改 `docs/命令行契约.md`。
3. Bump 相关 crate `version`；线协议 bump 时改 `SSE_PROTOCOL_VERSION` 与文档中的 **`` `N` ``** 标记（`crabmate-sse-protocol` 自检测试会读文档）。
4. `bash scripts/check-client-contract.sh`。
5. 打 `client-contract-vX.Y.Z`；更新壳仓钉 tag。

---

## 修订记录

| 日期 | 说明 |
|------|------|
| 2026-08-08 | Phase 1：semver / 线协议轴、N−1 现状、git tag 钉法、connect+Tauri 2、CI 门禁 |
