# Frontend 迁出实施计划（路径 A · Phase 4.2）

> **状态**：P4.2 / Phase A–C′ **已完成**（2026-08-08；主仓 #795、Client #2/#3）  
> **权威决策**：[`client_shell_split.md`](./client_shell_split.md)  
> **执行勾选**：[`client_shell_split_todo.md`](./client_shell_split_todo.md) Phase 4 · P4.2  
> **契约钉法**：[`client_contract_versioning.md`](./client_contract_versioning.md)、兼容表 [`client_compat_matrix.md`](./client_compat_matrix.md)  
> **展示 crate 下沉（后续）**：[`client_display_crate_sink.md`](./client_display_crate_sink.md)（`turn-layout` / `tool-card` 不再作为长期钉清单）  
> **Client 仓**：同级 / GitHub [`noisystreet/crabmate-client`](https://github.com/noisystreet/crabmate-client)（`docs/design/contract_pin.md`）

---

## 1. 目标与非目标

### 1.1 目标

1. **`frontend/` 源码**离开 Server 主仓，迁入 **Client 仓**（本计划默认落点）。
2. UI 仅经 **HTTP/SSE + 可钉契约 crate** 消费 `serve`；**禁止** Cargo `path` 回主开发树。
3. 主仓 `serve` 仍可**可选托管**预构建 `dist`（`CM_WEB_STATIC_DIR`）；官方 UI 构建与发版在 Client 侧。
4. 主仓 CI / pre-commit **不再**强制 `frontend` wasm 门禁（迁出完成后）。

### 1.2 非目标

- 不把回合执行、工具编排、密钥权威「下沉」进 `serve`（已在 Server）。
- 不强制本次拆独立 `CrabMate-ui` 仓（**Phase 5**）。
- 不要求 `cargo publish` 到 crates.io，也不要求先发完整 Server 产品版。
- 不把 Victauri / Playwright 全量搬进默认 Server CI。

---

## 2. 决策（已定 / 本计划采纳）

| 项 | 决定 |
|----|------|
| 落点 | **`crabmate-client/frontend/`**（与壳同仓；便于 `make desktop-release` / `CRABMATE_FRONTEND_DIST`） |
| 契约渠道 | 主仓 **git 注释标签** `client-contract-vX.Y.Z`（开发期可用 `rev`） |
| 是否先发产品版 | **否**；只需可钉的契约提交 |
| Server 默认行为（迁出后） | 文档与 Makefile 以 **`--no-web` 或显式 `CM_WEB_STATIC_DIR`** 为主叙事；过渡期可仍探测本地 dist（若存在） |
| connect | 已在 Client 仓；与 UI 无关 |

---

## 3. 依赖面（迁出前必须钉住）

`frontend/Cargo.toml` 当前 workspace path 依赖：

| Crate | 角色 | 进 `client-contract-v*`？ |
|-------|------|---------------------------|
| `crabmate-api-contract` | HTTP DTO / 错误码 | ✅ 已在钉清单 |
| `crabmate-sse-protocol` | SSE 版本与帧 | ✅ 已在钉清单 |
| `crabmate-types` | 传递 / 预设等 | ✅ 作为传递依赖至少可用；建议文档显式列出 |
| `crabmate-display-rules` | 展示过滤 | ✅ 传递；建议显式列出 |
| `crabmate-turn-layout` | 流式回合投影 | ⚠️ **须扩进钉清单**（当前 Phase 1 文档未写） |
| `crabmate-tool-card` | 工具卡展示 | ⚠️ **须扩进钉清单** |
| `crabmate-chat-export` | 导出会话 schema | ⚠️ **须扩进钉清单** |

**可选跟进（不阻塞首迁）**：`sse-protocol` 增加 WASM/`client` feature，避免拖 server tokio/hub 编译面。

**DTO 卫生（可并行）**：`frontend/src/api/http.rs`、`user_data.rs` 等本地镜像类型逐步改用 `api-contract`（含流式请求体少用手拼 `json!`）。不阻塞「目录搬家」，但建议在迁仓 PR 中至少列债务。

---

## 4. 阶段划分

### Phase A — 契约可钉（主仓，阻塞迁目录）

**入口**：P4.1 合入中/已合（壳双轨已删或本分支含 P4.1）。

| ID | 动作 | 验收 |
|----|------|------|
| A1 | 更新 [`client_contract_versioning.md`](./client_contract_versioning.md)：钉清单含 §3 表中全部 UI 依赖 crate | ✅ 文档与 `frontend/Cargo.toml` 对齐 |
| A2 | 更新 Client [`contract_pin.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/contract_pin.md) 示例 | ✅ 外仓可复制粘贴（随 Client 仓提交） |
| A3 | 扩展 `check-client-contract.sh`：钉清单 manifest + UI crate path 消费 smoke | ✅ 本地/CI `client-contract` 绿 |
| A4 | `main` 上打注释标签 **`client-contract-v0.1.0`**（或开发期约定 `rev`） | ✅ `client-contract-v0.1.0` → `c244ebb1` |

**不需要**：crates.io、`v0.1.0` 产品 Release、改用户安装包版本号。

### Phase B — 迁入 Client 仓（外仓）

| ID | 动作 | 验收 |
|----|------|------|
| B1–B5 | 见 Client [`feat/frontend-phase-b`](https://github.com/noisystreet/crabmate-client/pull/2) | ✅ 已合入 `main`（`b6f5dcc`，钉 `tag=client-contract-v0.1.0`） |

### Phase C — 主仓收尾（Server）

| ID | 动作 | 验收 |
|----|------|------|
| C1 | 根 `Cargo.toml` 去掉 `frontend` member；删除 `frontend/` 源码 | ✅ [#795](https://github.com/noisystreet/CrabMate/pull/795) |
| C2 | 去掉 pre-commit `frontend-wasm-check` / `frontend-clippy`；CI 去掉 wasm32 frontend 步骤 | ✅ |
| C3 | `Makefile`：`all` 仅为 `backend-release`；frontend 目标改为 Client 指针 | ✅ |
| C4 | `package-release.sh`：可选 UI dist；Playwright **迁 Client** | ✅ |
| C5 | `web_static_dir` / 文档：`CM_WEB_STATIC_DIR` / `--no-web` | ✅ |
| C6 | 更新 todo / 兼容表；勾选 P4.2 | ✅ |

### Phase C′ — Playwright 迁 Client（随 Phase C）

| ID | 动作 | 验收 |
|----|------|------|
| C7 | `e2e/` + workflow 迁 `crabmate-client`；主仓删除 Playwright 目录与 CI | ✅ Client [#3](https://github.com/noisystreet/crabmate-client/pull/3) + [#795](https://github.com/noisystreet/CrabMate/pull/795) |

### Phase D — 可选（P4.3 / Phase 5）

| ID | 动作 |
|----|------|
| D1 | Server release asset 附带推荐 UI tarball（源码仍不在主仓） |
| D2 | 独立 `CrabMate-ui` 仓：仅 trunk 产物；壳与浏览器同钉版本包 |

---

## 5. PR 切片建议

| PR | 仓 | 内容 |
|----|-----|------|
| 1 | CrabMate | Phase A：文档钉清单 +（可选）门禁扩展；**不删** frontend |
| 2 | CrabMate | 打 tag `client-contract-v0.1.0`（可在 PR1 合入后操作，不一定是 PR） |
| 3 | crabmate-client | Phase B：迁入 frontend + git 依赖 + CI |
| 4 | CrabMate | Phase C：删 frontend、改 CI/Makefile/文档 |

避免「单 PR 又迁目录又删主仓又改契约」导致回滚困难。

---

## 6. 风险与缓解

| 风险 | 缓解 |
|------|------|
| git tag 拉取 workspace 慢 / 解析失败 | 先用 `rev` 验证；tag 仅钉契约相关 crate 所在提交 |
| `sse-protocol` 编译进 WASM 过重 | 后续 `client` feature；首迁可接受若 CI 能编过 |
| Playwright 曾假设 monorepo `frontend/dist` | ✅ 已迁 Client `e2e/` + checkout Server 编 serve |
| 双仓短暂双份 frontend | 主仓删源码前冻结主仓 frontend 改动；或主仓改 README「只读指向 Client」 |
| 契约 crate 破坏性变更 | 走 `SSE_PROTOCOL_VERSION` / semver 与兼容表；外仓 bump tag |

---

## 7. 验收清单（P4.2 总验收）

- [x] Client 仓可 `trunk build`（仅 git tag/rev，无 path 回主仓）
- [x] Desktop/浏览器：静态 UI + 远程或本机 `serve` 一轮对话（Bearer + SSE v2）（冒烟 runbook / 人工验收）
- [x] 主仓无 `frontend/` 源码；CI 无强制 wasm UI
- [x] `serve --no-web` 或 `CM_WEB_STATIC_DIR` 文档可跟随操作
- [x] [`client_compat_matrix.md`](./client_compat_matrix.md) 补一行：Server 契约 tag ↔ 最低 UI
- [x] todo P4.2 勾选

---

## 8. 建议执行顺序（相对当前分支）

1. 推送并合入 **P4.1**（`chore/client-shell-phase4`）。
2. 开 **Phase A** PR（本文档可随 A1 入库）。
3. 打 **`client-contract-v0.1.0`**（或先 `rev` 联调）。
4. Client **Phase B** → 主仓 **Phase C** → Client Playwright。

**已完成**（2026-08-08）：上列 1–4 均已合入 `main`。

---

## 9. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-08-08 | 初稿：落点 Client 仓；先契约 tag/rev，不强制产品发版；A/B/C/D 阶段与 PR 切片 |
| 2026-08-08 | Phase A 落地：扩钉清单、门禁 smoke、Client `contract_pin`；A4 待合 main 后打 tag |
| 2026-08-08 | Playwright E2E 迁 Client；主仓仅保留转发脚本 |
| 2026-08-08 | P4.2 / Phase B+C+C′ 合入完成；勾选总验收 |
