# ADR：官方 Client 与本仓「只维护 Server」（路径 A）

> **状态**：**已采纳（2026-08-08）**；**2026-08-10 修订** — 官方终端改为 Client 远程 `crabmate-tui`；同进程 `chat|repl|tui` 降为过渡。  
> **执行清单**：[`client_shell_split_todo.md`](./client_shell_split_todo.md)  
> **契约发版（Phase 1）**：[`client_contract_versioning.md`](./client_contract_versioning.md)  
> **运行时 UI/API 拆分**：[`client_ui_runtime_split.md`](./client_ui_runtime_split.md)（`serve` 默认纯 API）  
> **远程 CLI/TUI（Client）**：同级 [`../crabmate-client/docs/design/remote_cli_tui.md`](../../../crabmate-client/docs/design/remote_cli_tui.md)（GitHub：[remote_cli_tui.md](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/remote_cli_tui.md)）  
> **关联**：[`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)、[`crate_dep_policy.md`](./crate_dep_policy.md)、[`web_host_extract.md`](./web_host_extract.md)、[`turn_runtime_placement.md`](./turn_runtime_placement.md)、[`web_host_p5_placement.md`](./web_host_p5_placement.md)；契约见 **`docs/SSE协议.md`**、**`docs/命令行与路由.md`**、**`docs/配置说明.md`**。  
> **非目标**：拆 Agent 微服务；另开第二套会话 API；把同进程 `run_agent_turn` 剪贴进 Client；立刻删除本仓 `crabmate chat|repl|tui`；多租户账号体系；以本 ADR 替代 turn-runtime / queue 搬家决策。

---

## 1. 背景

CrabMate **执行权威**在 **`crabmate serve`**（Agent / 工具 / 工作区）。桌面 / 移动壳已**不** spawn sidecar；业务 UI 已迁 Client，经 API 基址 + CORS 连接 `serve`。同进程 **`repl` / `tui` / `chat`** 仍可在本仓二进制内直调 `run_agent_turn`，但与「只维护 Server、官方 Client 只认 HTTP/SSE」的目标句不一致。

目标句：**本仓只维护 Server**；官方 Client（Desktop Linux、Android、浏览器直连、**远程终端**）可独立发版，只认稳定 HTTP/SSE。

---

## 2. 决策

### 2.1 路径 A（采纳）

| 项 | 决定 |
|----|------|
| 业务 UI | 由 Client / 可选独立 UI 仓构建与发版；经可配置 **API 基址** 连接兼容的 `serve` |
| 路径 B（UI 永远随 server 托管） | **不作终点**；不得用「UI 随 server」宣称「只维护 server」已完成（勿与下文「远程终端路径 B」混淆） |
| 本仓终点 | `serve` + 契约 crate；**官方终端不在本仓一等公民**；`frontend/` 源码已迁出（壳 Phase 4.1；UI Phase 4.2） |
| 同进程 CLI/TUI | **过渡保留**（`crabmate chat|repl|tui` 仍可同进程执行）；**不**迁入 Client；日后 deprecate / 删除时间表另定 |
| 拆壳仓门槛 | **须先**完成契约可发布 + 前端 API 基址/CORS（见 todo Phase 1–2）；禁止跳过 Phase 2 |

### 2.2 官方 Client 矩阵

| 入口 | 形态 | 备注 |
|------|------|------|
| **Desktop Linux** | Tauri 壳（**`../crabmate-client/desktop-tauri/`**） | 回环可保留有限 IPC；非回环 IPC 降级可接受 |
| **Android** | Tauri 壳（**`../crabmate-client/mobile-tauri/`**） | 不 spawn 本机 Agent |
| **浏览器直连** | 静态托管官方 WASM/UI | 与壳共用同一套 UI 产物或同源构建 |
| **Terminal（远程）** | Client 仓 **`crabmate-tui`**（HTTP + SSE） | 与壳同契约钉 / Web Bearer / `client_llm`；**不**内嵌、**不** spawn `serve`；分期见 Client [`remote_cli_tui.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/remote_cli_tui.md) |

**不在官方矩阵**（可存在但不承诺一等公民）：macOS/Windows 桌面、iOS、IDE 扩展、IM 桥等——另开产品切片，仍只认同一契约。

**本仓同进程 `chat` / `repl` / `tui`**：**过渡期**仍可提供；**不是**上表一等入口。文档与 help 应逐步指向 **`crabmate-tui`**；删除窗口见 Client 文 **P5** 与后续 Server 公告。

### 2.3 远程终端（Client 路径 B，已拍板）

与「UI 永远随 server」的路径 B **无关**。Client 侧决定：官方终端为**远程客户端**（同 Tauri 壳），执行权威仅 `serve`。

| 项 | 决定 |
|----|------|
| 二进制 | **`crabmate-tui`**（避免与本仓 `crabmate` / Desktop `crabmate-desktop` 冲突） |
| 契约 | 钉 `crabmate-sse-protocol` / `crabmate-api-contract` 等（同 `frontend`） |
| 模型密钥 | 本机钥匙串 → 请求体 **`client_llm.api_key`**（同 WASM UI）；**不**依赖本仓进程内 `/api-key` 作为官方路径 |
| 同进程 Agent CLI | **不**迁入 Client；本仓可暂留 |

细节、分期（P0–P5）与仓内布局以 Client **`remote_cli_tui.md`** 为准；本 ADR 只钉**产品边界**。

### 2.4 密钥边界（不变）

| 密钥 | 用途 | 禁止 |
|------|------|------|
| **Web API 共享密钥**（`CM_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`；Client 连接页/侧栏/钥匙串） | 保护 HTTP API（`Authorization: Bearer` / `X-API-Key`） | 不得当作模型密钥发给上游 LLM |
| **模型 `API_KEY` / `client_llm`** | 上游 `chat/completions` 等 | 不得要求浏览器把模型密钥当 Web Bearer；日志/错误体不得打印完整值 |
| 其它（GitHub、MCP bearer 等） | 各能力专用 | 不与上两行混用 |

跨 Origin 仍只靠 **Web Bearer** + CORS 白名单；connect hash（`#cm_web_api_bearer=`）仅传递 Web API 密钥；非回环监听策略与现文档一致。远程终端与壳相同：Bearer 鉴权 API；模型密钥走 `client_llm`（及钥匙串合并），见 **`docs/配置说明.md`**。

---

## 3. 选项对比

| 选项 | 做法 | 收益 | 代价 |
|------|------|------|------|
| **A. Client 自带 UI（采纳）** | API 基址 + CORS；UI 随 Client/UI 仓发版；本仓可去 `frontend/` 源码 | 真正「只维护 server」；多端 Client 同契约可独立发版 | 跨 Origin、发版矩阵、双仓 CI 更复杂 |
| B. UI 随 Server | 壳只连 URL；UI 始终由 `serve` 托管 dist | 改动小；同 Origin 简单 | 本仓仍养 `frontend/`；发版边界未断 |
| C. 仅拆壳仓、UI 仍同源 | 先迁 Tauri 目录 | 主仓 CI 可甩掉 GTK | 易偷换成「假完成」；与目标句冲突 |

**终端面**（Client 已拍板，相对本仓同进程 CLI）：

| 选项 | 做法 | 本仓立场 |
|------|------|----------|
| **远程 `crabmate-tui`（采纳）** | Client 仓 HTTP/SSE 客户端 | 官方矩阵入口；与路径 A 一致 |
| 同进程 `repl`/`tui` 迁入 Client | 剪贴 `run_agent_turn` / 工具栈 | **拒绝**（非目标） |
| 立刻删除本仓 `chat|repl|tui` | 只留 `serve` | **拒绝（现阶段）**；过渡期并行后再 deprecate |

---

## 4. 后果与约束

1. **契约优先**：不新开会话 API；Client（含 `crabmate-tui`）只认现有 HTTP + SSE（见 `docs/SSE协议.md`）。
2. **与宿主解耦正交**：`turn_runtime_placement` / `web_host_p5_placement` **不阻塞**本决策；拆壳优先传输与发版边界。
3. **现状**：壳目录仅在 **`../crabmate-client`**（Phase 4.1）；业务 UI 与 Playwright 亦在 Client（Phase 4.2 / [#795](https://github.com/noisystreet/CrabMate/pull/795)、Client [#2](https://github.com/noisystreet/crabmate-client/pull/2)/[#3](https://github.com/noisystreet/crabmate-client/pull/3)）。Phase 2 已提供 **API 基址 + CORS**。兼容表见 [`client_compat_matrix.md`](./client_compat_matrix.md)。远程终端分期见 Client `remote_cli_tui.md`（P2 已落地 `chat|repl` + 审批）。
4. **安全**：CORS 默认保守（空白名单=不挂层；启用时精确 Origin）；非回环须 Bearer。
5. **本仓文档债**：`命令行与路由` / README / man / help 须标明官方终端为 **`crabmate-tui`**、同进程入口为过渡；删除时间表与 Client **P5** 对齐后再动代码。

---

## 5. 成功标准

**壳仓与远程终端只认已发布的协议版本与 API 基址，并自带业务 UI / 终端面；主仓只发布 `serve`（与契约）——路径 A 为唯一终点。同进程 CLI/TUI 不计入路径 A 完成条件。**

分阶段验收与勾选见 [`client_shell_split_todo.md`](./client_shell_split_todo.md)；远程终端验收以 Client **`remote_cli_tui.md`** 分期为准。
