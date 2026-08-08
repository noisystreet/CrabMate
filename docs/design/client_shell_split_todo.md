# 官方 Client 拆分 — 执行计划（路径 A）

> **权威决策**：[`client_shell_split.md`](./client_shell_split.md)（ADR）  
> **日期**：2026-08-08  
> **约定**：完成某阶段后更新本文件勾选；**勿**把本地草稿目录当作本计划的引用源（见根目录 `AGENTS.md`）。  
> **关联**：[`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)、`docs/SSE协议.md`、`docs/配置说明.md`、`desktop-tauri/README.md`、`mobile-tauri/README.md`

---

## 进度总览

| 阶段 | 主题 | 状态 |
|------|------|------|
| Phase 0 | 决策与基线 | ✅ 完成（2026-08-08） |
| Phase 1 | 契约可发布 | ⬜ 未开始 |
| Phase 2 | 前端可远程（API 基址 + CORS） | ⬜ 未开始 |
| Phase 3 | connect + 壳仓拆出 | ⬜ 未开始（须 Phase 1+2） |
| Phase 4 | 本仓收尾（去壳 / 移出 frontend 源码） | ⬜ 未开始 |
| Phase 5 | （可选）独立 UI 仓 | ⬜ 按需 |

**下一执行**：Phase 1。

---

## 阻塞项（对照）

| ID | 阻塞 | 解除阶段 |
|----|------|----------|
| K1 | `frontend` 与协议/契约 crate 同仓 path 编译 | Phase 2–4 |
| K2 | 前端相对路径、假定同 Origin | Phase 2 |
| K3 | 壳 → `crabmate-connect` path | Phase 1+3 |
| K4 | 缺可独立发版的 semver/兼容表 | Phase 1 |
| K5 | E2E 假设 monorepo 同树 | Phase 3 |
| K6 | 桌面非回环无完整 IPC | 文档化即可（非硬阻塞） |

**已具备**：壳不 spawn `serve`；契约 crate 与金样；`request_id` / 可读错误；`crabmate-connect` 在主 workspace 外。

---

## Phase 0 — 决策与基线 ✅

- [x] 选定路径 **A**（见 ADR §2.1）
- [x] 官方 Client 矩阵：Desktop Linux、Android、浏览器直连（ADR §2.2）
- [x] 密钥边界不变（ADR §2.3）
- [ ] `desktop-tauri` / `mobile-tauri` README 与路径 A 终点对齐（**延后到 Phase 2/4**）

---

## Phase 1 — 契约可发布

**入口**：Phase 0 决策项完成。  
**解开**：K3/K4 准备。

| ID | 动作 | 验收提示 |
|----|------|----------|
| P1.1 | `crabmate-api-contract`、`crabmate-sse-protocol`：**semver** + 破坏性变更策略（含 `SSE_PROTOCOL_VERSION`） | 版本策略写进 `docs/SSE协议.md` / 命令行契约 |
| P1.2 | CI：金样 + OpenAPI 漂移保持绿；文档写清 N / N-1 兼容窗口（若有） | CI 绿 |
| P1.3 | crate：`cargo publish` **或** 固定 git tag 依赖说明；文档化壳仓如何钉版本 | 外仓可不经 monorepo path 依赖（试验仓或 CI job） |
| P1.4 | `crabmate-connect`：同样 semver/tag；与 Tauri 2 兼容说明 | 同上 |

**PR 建议**：① docs 发版策略 ② chore 版本/（可选）publish ③ 不改壳业务行为

- [ ] P1.1  
- [ ] P1.2  
- [ ] P1.3  
- [ ] P1.4  
- [ ] Phase 1 总验收：外仓钉版本依赖契约 crate；协议错位错误码仍可预期  

---

## Phase 2 — 前端可远程（路径 A 关键）

**入口**：Phase 1 验收。  
**解开**：K1/K2。

| ID | 动作 | 验收提示 |
|----|------|----------|
| P2.1 | 前端可配置 **API 基址**（构建期 env 或运行时；默认空 = 同 Origin） | 默认同 Origin 零回归 |
| P2.2 | `frontend/src/api/**` 与 SSE 走基址；`serve` 可配 **CORS**（保守默认） | 跨 Origin 手工或 runbook 勾选 |
| P2.3 | 跨 Origin 仅 Web Bearer；不引入模型密钥鉴权 Web | 与 ADR §2.3 一致 |
| P2.4 | 文档：静态托管 UI + 远程 serve；非回环须 Bearer | `docs/` + README |
| P2.5 | 跨 Origin 冒烟写入 [`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md) 可选节 | 浏览器直连一轮对话 |

**风险**：CORS 过宽 → 默认拒绝或 Origin 白名单。

**PR 建议**：① feat(frontend) API base ② feat(api) CORS ③ docs/runbook ④ 测试

- [ ] P2.1–P2.5  
- [ ] Phase 2 总验收：静态 UI + 远程 serve + Bearer 可聊；`serve` 托管 dist 默认行为不变  

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

- [ ] P3.1–P3.5  
- [ ] 验收：干净克隆壳仓可 release；连本仓 `serve` 一轮对话（Desktop 与 Android 至少一端）；主仓可不强制 desktop GTK job  

---

## Phase 4 — 本仓收尾

**入口**：Phase 3；且 Phase 2 已验收。

| ID | 动作 |
|----|------|
| P4.1 | 主仓移除 `desktop-tauri/`、`mobile-tauri/` 及无用打包脚本 |
| P4.2 | `frontend/` 源码迁出到壳仓或 UI 仓；`serve` 默认 `--no-web` 或最小占位 / 文档链到 UI 发版物 |
| P4.3 | （可选）release asset 附带推荐 UI 包，**源码**不在主仓 |
| P4.4 | 兼容表：Server ↔ 协议版 ↔ 最低 Client（写入 `docs/`） |
| P4.5 | 评估 `serve --desktop-ready-json` 改名/废弃 |

- [ ] P4.1–P4.5  
- [ ] 验收：主仓构建无强制 Tauri/GTK；官方安装包来自壳仓；兼容表齐套；壳 README 与路径 A 一致  

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
