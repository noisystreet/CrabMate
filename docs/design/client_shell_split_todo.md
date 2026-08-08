# 官方 Client 拆分 — 执行计划（路径 A）

> **权威决策**：[`client_shell_split.md`](./client_shell_split.md)（ADR）  
> **契约发版（Phase 1）**：[`client_contract_versioning.md`](./client_contract_versioning.md)  
> **日期**：2026-08-08  
> **约定**：完成某阶段后更新本文件勾选；**勿**把本地草稿目录当作本计划的引用源（见根目录 `AGENTS.md`）。  
> **关联**：[`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)、[`client_compat_matrix.md`](./client_compat_matrix.md)、`docs/SSE协议.md`、`docs/配置说明.md`；壳 README / 冒烟 / Victauri：**仅** **`../crabmate-client`**（Phase 4.1 起本仓已无壳目录）

---

## 进度总览

| 阶段 | 主题 | 状态 |
|------|------|------|
| Phase 0 | 决策与基线 | ✅ 完成（2026-08-08） |
| Phase 1 | 契约可发布 | ✅ 完成（2026-08-08） |
| Phase 2 | 前端可远程（API 基址 + CORS） | ✅ 完成（2026-08-08） |
| Phase 3 | connect + 壳仓拆出 | ✅ 完成（2026-08-08；见下方总验收） |
| Phase 4 | 本仓收尾（去壳 / 移出 frontend 源码） | ✅ **完成**（P4.1–P4.2、P4.4–P4.5；可选 P4.3） |
| Phase 5 | （可选）独立 UI 仓 | ⬜ 按需 |

**下一执行**：可选 P4.3（release 附 UI 包）或 Phase 5；日常按兼容表发版。

---

## 阻塞项（对照）

| ID | 阻塞 | 解除阶段 |
|----|------|----------|
| K1 | `frontend` 与协议/契约 crate 同仓 path 编译 | ✅ Phase 4.2（UI 在 Client，仅 git tag/rev） |
| K2 | 前端相对路径、假定同 Origin | ✅ Phase 2（可配 API 基址；默认同 Origin） |
| K3 | 壳 → `crabmate-connect` path | ✅ Phase 3（connect 在外仓 path；契约钉 tag 文档+门禁） |
| K4 | 缺可独立发版的 semver/兼容表 | ✅ Phase 1（见 `client_contract_versioning.md`） |
| K5 | E2E 假设 monorepo 同树 | ✅ Phase 3（外仓脚本 + 外部 `serve`；Victauri DOM 稳定性另跟） |
| K6 | 桌面非回环无完整 IPC | 文档化即可（非硬阻塞） |

**已具备**：壳不 spawn `serve`；契约 crate 与金样；`request_id` / 可读错误；**`crabmate-connect` 仅在 Client 仓**（Phase 4.1）；Phase 1 契约发版；**Phase 2**：API 基址 + 保守 CORS + runbook §9。

---

## Phase 0 — 决策与基线 ✅

- [x] 选定路径 **A**（见 ADR §2.1）
- [x] 官方 Client 矩阵：Desktop Linux、Android、浏览器直连（ADR §2.2）
- [x] 密钥边界不变（ADR §2.3）
- [x] 壳 README 与路径 A 终点对齐（权威在 **`../crabmate-client`**；Phase 4.1 本仓已移除壳副本）

---

## Phase 1 — 契约可发布 ✅

**入口**：Phase 0 决策项完成。  
**解开**：K4；K3 钉法准备（拆仓改 tag 仍属 Phase 3）。

| ID | 动作 | 验收提示 |
|----|------|----------|
| P1.1 | `crabmate-api-contract`、`crabmate-sse-protocol`：**semver** + 破坏性变更策略（含 `SSE_PROTOCOL_VERSION`） | [`client_contract_versioning.md`](./client_contract_versioning.md) + `docs/SSE协议.md` / `docs/命令行契约.md` |
| P1.2 | CI：金样 + OpenAPI 漂移保持绿；文档写清 N / N-1 兼容窗口（若有） | `scripts/check-client-contract.sh`；CI job **`client-contract`**；文档 §3 写明线协议**当前无** N−1 解码窗口 |
| P1.3 | crate：`cargo publish` **或** 固定 git tag 依赖说明；文档化壳仓如何钉版本 | **默认 git tag** `client-contract-vX.Y.Z`（暂不强制 crates.io）；脚本内 path 消费冒烟 |
| P1.4 | `crabmate-connect`：同样 semver/tag；与 Tauri 2 兼容说明 | Client 仓 `crates/crabmate-connect/README.md` + versioning §5 |

**PR 建议**：① docs 发版策略 ② chore 版本/门禁 ③ 不改壳业务行为

- [x] P1.1  
- [x] P1.2  
- [x] P1.3  
- [x] P1.4  
- [x] Phase 1 总验收：外仓可按文档钉 git tag（或 `rev`）依赖契约 crate；协议错位错误码仍可预期；`bash scripts/check-client-contract.sh` 绿  

---

## Phase 2 — 前端可远程（路径 A 关键） ✅

**入口**：Phase 1 验收。  
**解开**：K2；K1 部分（可远程消费，源码迁出仍属 Phase 4）。

| ID | 动作 | 验收提示 |
|----|------|----------|
| P2.1 | 前端可配置 **API 基址**（构建期 `CRABMATE_API_BASE` 或运行时 localStorage；默认空 = 同 Origin） | 默认同 Origin 零回归 |
| P2.2 | `frontend/src/api/**` 与 SSE 走基址；`serve` 可配 **CORS**（`web_cors_allowed_origins` / `CM_WEB_CORS_ALLOWED_ORIGINS`；空=不挂） | 跨 Origin：runbook §9 |
| P2.3 | 跨 Origin 仅 Web Bearer；不引入模型密钥鉴权 Web | 与 ADR §2.3 一致 |
| P2.4 | 文档：静态托管 UI + 远程 serve；非回环须 Bearer | `docs/配置说明.md`、README、命令行与路由 |
| P2.5 | 跨 Origin 冒烟写入 [`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md) §9 | 浏览器直连一轮对话 |

**风险**：CORS 过宽 → 默认拒绝（空白名单）；仅精确 Origin。

**PR 建议**：① feat(frontend) API base ② feat(api) CORS ③ docs/runbook ④ 测试

- [x] P2.1–P2.5  
- [x] Phase 2 总验收：静态 UI + 远程 serve + Bearer 可聊（runbook §9）；`serve` 托管 dist 且 API 基址空时默认行为不变  

---

## Phase 3 — Connect + 壳仓拆出

**入口**：**Phase 1 与 Phase 2 均须通过**（禁止无 API 基址时拆壳并自称完成）。  
**解开**：K3/K5。

| ID | 动作 |
|----|------|
| P3.1 | 新仓（如 `CrabMate-desktop` / `CrabMate-mobile` 或 `CrabMate-clients`） |
| P3.2 | 迁入壳目录 + `crabmate-connect`；**业务 UI 进壳或依赖 UI 产物** |
| P3.3 | 依赖改为 crates.io / git tag（禁止 path 回主仓） |
| P3.4 | 壳仓 CI；E2E 对已发布/已安装的 `serve`（钉协议版本） |
| P3.5 | 主仓 README 指向外仓；目录删除或短期 submodule（见 Phase 4） |

- [x] P3.1：本地外仓 **`../crabmate-client`**（相对本仓；2026-08-08）
- [x] P3.2：壳 + connect + 业务 UI 均在 Client 仓；壳导航远程 `serve` UI；可选 `CRABMATE_FRONTEND_DIST`
- [x] P3.2 文档：壳设计 / 冒烟 / TESTING / AGENTS+pre-commit 在外仓；主仓 `tauri_gui_mvp_design.md` 为指针
- [x] P3.3：壳对 connect 为 Client 仓 path；**禁止** path 回主仓（`scripts/check-no-main-path.sh` + CI）；契约钉法见外仓 `docs/design/contract_pin.md`（首枚 `client-contract-v*` 仍待主仓打 tag）
- [x] P3.4：壳仓 CI（`.github/workflows/ci.yml`：fmt/clippy/test；Victauri 全量 E2E 不进默认 CI）
- [x] P3.5：主仓 README / 壳 README 指向外仓；壳目录于 Phase 4.1 从主仓移除
- [x] 验收（2026-08-08）：
  - **干净克隆 release**：`git clone` → `/tmp/crabmate-client-p3-accept`（分支 `ci/makefile-and-deb-package`）→ `make desktop-release` → `crabmate_0.1.0_amd64.deb`；`dpkg-deb` 含 `usr/bin/crabmate-desktop`、**无** `usr/bin/crabmate` sidecar
  - **Desktop + 本仓 serve 一轮对话**：主仓 `serve`（`CM_WEB_STATIC_DIR=../crabmate-client/frontend/dist`，`:18080`）托管 UI；干净克隆 **release** `crabmate-desktop` 以 `CM_DESKTOP_SKIP_CONNECT` + `#cm_web_api_bearer=` 打开该 URL；同进程 `POST /chat/stream`（`client_sse_protocol=2`）提示词「用一句话介绍你自己」→ HTTP 200 + SSE 助手正文。协议错位：`client_sse_protocol=99` → `SSE_CLIENT_TOO_NEW`
  - **主仓可不强制 desktop GTK job**：路径 A 下 Server CI **不必**以 GTK/桌面为硬门禁（Phase 4.1 起本仓已无 desktop job）

---

## Phase 4 — 本仓收尾 ✅

**入口**：Phase 3；且 Phase 2 已验收。

| ID | 动作 | 状态 |
|----|------|------|
| P4.1 | 主仓移除 `desktop-tauri/`、`mobile-tauri/`、`crates/crabmate-connect/` 及无用打包脚本（含 `scripts/victauri-e2e.sh`、`scripts/sync-tauri-connect-page.sh`） | ✅ 完成 |
| P4.2 | `frontend/` 源码迁出到壳仓或 UI 仓；`serve` 默认 `--no-web` 或最小占位 / 文档链到 UI 发版物 | ✅ [CrabMate#795](https://github.com/noisystreet/CrabMate/pull/795) + Client [#2](https://github.com/noisystreet/crabmate-client/pull/2) / [#3](https://github.com/noisystreet/crabmate-client/pull/3) |
| P4.3 | （可选）release asset 附带推荐 UI 包，**源码**不在主仓 | ⬜ 待做 |
| P4.4 | 兼容表：Server ↔ 协议版 ↔ 最低 Client | ✅ 初稿 [`client_compat_matrix.md`](./client_compat_matrix.md) |
| P4.5 | `serve --desktop-ready-json`：保留旗标；新增别名 **`--web-ready-json`**；壳不依赖；文档标注弃用命名 | ✅ 完成 |

- [x] P4.1  
- [x] P4.2  
- [ ] P4.3  
- [x] P4.4（初稿）  
- [x] P4.5  
- [x] 验收：主仓构建无强制 Tauri/GTK；官方安装包来自壳仓；兼容表齐套；壳 README 与路径 A 一致；**`frontend/` 源码不在本仓**；Playwright 在 Client  

---

## Phase 5 — （可选）独立 UI 仓

利于 Desktop / Android / 浏览器共用同一 WASM。

| ID | 动作 |
|----|------|
| P5.1 | `CrabMate-ui`：`trunk` 产出版本化静态包 |
| P5.2 | 壳只做 WebView + connect；浏览器直连该静态包 |
| P5.3 | Server 可选：推荐 UI 版本或包 URL |

- [ ] Phase 5（按需）  

---

## 与宿主解耦

| 主题 | 态度 |
|------|------|
| turn-runtime crate | 不阻塞；维持暂不建 |
| queue 迁 web-host | 不阻塞 |
| Facet 收窄 | 有益，非本计划门禁 |
| SSE 外存 | 另开 |

---

## 时间盒（弹性，单人熟悉代码）

| 阶段 | 量级 |
|------|------|
| Phase 1 | 2–5 日 |
| Phase 2 | 5–10 日 |
| Phase 3 | 3–7 日 |
| Phase 4 | 2–4 日 |
| Phase 5 | 按需 |

---

## 修订记录

| 日期 | 说明 |
|------|------|
| 2026-08-08 | 自中间稿升格；路径 A / Client 矩阵 / 密钥边界已决；Phase 0 完成 |
| 2026-08-08 | Phase 1：`client_contract_versioning.md`、门禁脚本与 CI job `client-contract`；下一刀 Phase 2 |
| 2026-08-08 | Phase 2：API 基址 + `web_cors_allowed_origins` + runbook §9；下一刀 Phase 3 |
| 2026-08-08 | Phase 2 补丁：CORS **expose** 会话头；`/uploads` CORP 仅 CORS 启用时放宽；API 基址显式清空不回落 `CRABMATE_API_BASE` |
| 2026-08-08 | Phase 3 开工：本地外仓 `../crabmate-client`（壳 + connect）；主仓目录暂保留双轨 |
| 2026-08-08 | Phase 3 文档：壳专题 / 冒烟 / Victauri / AGENTS+pre-commit 迁入 `crabmate-client`；主仓 `tauri_gui_mvp` 改指针 |
| 2026-08-08 | Phase 3 P3.3–P3.5：外仓 CI + `check-no-main-path` + contract_pin；主仓 README/壳 README 指向外仓 |
| 2026-08-08 | Phase 3 总验收：干净克隆 `.deb` + Desktop release 壳对接本仓 UI/`serve` 真实 SSE 回合；下一刀 Phase 4 |
| 2026-08-08 | Phase 4.1：移除主仓壳/`connect`/Victauri 脚本与 desktop CI；P4.4 兼容表初稿；P4.5 `--web-ready-json`；下一刀 P4.2 迁 `frontend` |
| 2026-08-08 | P4.2 实施计划草案：[`frontend_migrate_plan.md`](./frontend_migrate_plan.md)（Client 仓落点；先 `client-contract-v*`，不强制产品发版） |
| 2026-08-08 | Phase A：扩契约钉清单 + `check-client-contract` UI smoke；Client `contract_pin`；tag 待合 main |
| 2026-08-08 | Phase 4.1：主仓移除壳 / connect / Victauri 脚本；P4.4 兼容表初稿；P4.5 `--web-ready-json` 别名；下一刀 **P4.2**（迁 `frontend`） |
| 2026-08-08 | **P4.2 完成**：Client #2（UI）+ #3（Playwright）与主仓 #795（删 frontend / e2e）已合 `main`；路径 A 分离终点达成（可选 P4.3 / Phase 5） |
