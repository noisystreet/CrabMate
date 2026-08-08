# ADR：官方 Client 与本仓「只维护 Server」（路径 A）

> **状态**：**已采纳（2026-08-08）** — 官方业务 UI 由 **Client 侧**构建与发版；本仓终点态以 **`serve` + 契约 +（可选）CLI/TUI** 为主，**不**以维护 `frontend/` 源码为产品职责。  
> **执行清单**：[`client_shell_split_todo.md`](./client_shell_split_todo.md)  
> **关联**：[`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)、[`crate_dep_policy.md`](./crate_dep_policy.md)、[`web_host_extract.md`](./web_host_extract.md)、[`turn_runtime_placement.md`](./turn_runtime_placement.md)、[`web_host_p5_placement.md`](./web_host_p5_placement.md)；契约见 **`docs/SSE协议.md`**、**`docs/命令行与路由.md`**、**`docs/配置说明.md`**。  
> **非目标**：拆 Agent 微服务；另开第二套会话 API；强制 CLI/TUI 走 HTTP；多租户账号体系；以本 ADR 替代 turn-runtime / queue 搬家决策。

---

## 1. 背景

CrabMate 执行权威在单机 **`serve`**（或同进程 CLI/TUI）。桌面 / 移动壳已**不** spawn sidecar，但官方 UI 仍多由目标 `serve` **同源托管** `frontend/dist`，且 `frontend` 与 `crabmate-sse-protocol` / `api-contract` **同仓 path 编译**。因此「薄壳」只解了进程生命周期，**未**解发版边界。

目标句：**本仓只维护 Server**；官方 Client（Desktop Linux、Android、浏览器直连）可独立发版，只认稳定 HTTP/SSE。

---

## 2. 决策

### 2.1 路径 A（采纳）

| 项 | 决定 |
|----|------|
| 业务 UI | 由 Client / 可选独立 UI 仓构建与发版；经可配置 **API 基址** 连接兼容的 `serve` |
| 路径 B（UI 永远随 server 托管） | **不作终点**；不得用 B 宣称「只维护 server」已完成 |
| 本仓终点 | `serve` + 契约 crate +（可选）CLI/TUI；`frontend/` 源码迁出（过渡期可短期双轨，不得无限期） |
| 拆壳仓门槛 | **须先**完成契约可发布 + 前端 API 基址/CORS（见 todo Phase 1–2）；禁止跳过 Phase 2 |

### 2.2 官方 Client 矩阵

| 入口 | 形态 | 备注 |
|------|------|------|
| **Desktop Linux** | Tauri 壳（现 `desktop-tauri/`） | 回环可保留有限 IPC；非回环 IPC 降级可接受 |
| **Android** | Tauri 壳（现 `mobile-tauri/`） | 不 spawn 本机 Agent |
| **浏览器直连** | 静态托管官方 WASM/UI | 与壳共用同一套 UI 产物或同源构建 |

**不在官方矩阵**（可存在但不承诺一等公民）：macOS/Windows 桌面、iOS、IDE 扩展、IM 桥等——另开产品切片，仍只认同一契约。

**CLI / TUI**：同进程宿主，**不是**上表 Client；本仓可继续提供。

### 2.3 密钥边界（不变）

| 密钥 | 用途 | 禁止 |
|------|------|------|
| **Web API 共享密钥**（`CM_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`；Client 连接页/侧栏/钥匙串） | 保护 HTTP API（`Authorization: Bearer` / `X-API-Key`） | 不得当作模型密钥发给上游 LLM |
| **模型 `API_KEY` / `client_llm`** | 上游 `chat/completions` 等 | 不得要求浏览器把模型密钥当 Web Bearer；日志/错误体不得打印完整值 |
| 其它（GitHub、MCP bearer 等） | 各能力专用 | 不与上两行混用 |

跨 Origin 仍只靠 **Web Bearer** + CORS 白名单；connect hash（`#cm_web_api_bearer=`）仅传递 Web API 密钥；非回环监听策略与现文档一致。

---

## 3. 选项对比

| 选项 | 做法 | 收益 | 代价 |
|------|------|------|------|
| **A. Client 自带 UI（采纳）** | API 基址 + CORS；UI 随 Client/UI 仓发版；本仓可去 `frontend/` 源码 | 真正「只维护 server」；三端 Client 同契约可独立发版 | 跨 Origin、发版矩阵、双仓 CI 更复杂 |
| B. UI 随 Server | 壳只连 URL；UI 始终由 `serve` 托管 dist | 改动小；同 Origin 简单 | 本仓仍养 `frontend/`；发版边界未断 |
| C. 仅拆壳仓、UI 仍同源 | 先迁 Tauri 目录 | 主仓 CI 可甩掉 GTK | 易偷换成「假完成」；与目标句冲突 |

---

## 4. 后果与约束

1. **契约优先**：不新开会话 API；Client 只认现有 HTTP + SSE（见 `docs/SSE协议.md`）。
2. **与宿主解耦正交**：`turn_runtime_placement` / `web_host_p5_placement` **不阻塞**本决策；拆壳优先传输与发版边界。
3. **现状差距（过渡）**：今日 `desktop-tauri` / `mobile-tauri` README 仍描述「加载 serve 的 UI」——属路径 B 现状；须在 todo Phase 2/4 改齐，不得与终点混淆。
4. **安全**：CORS 默认保守（回环或显式 Origin 白名单）；非回环须 Bearer。

---

## 5. 成功标准

**壳仓只认已发布的协议版本与 API 基址，并自带业务 UI；主仓只发布 `serve`（与契约）——路径 A 为唯一终点。**

分阶段验收与勾选见 [`client_shell_split_todo.md`](./client_shell_split_todo.md)。
