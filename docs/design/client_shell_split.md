# ADR：官方 Client 与本仓「只维护 Server」（路径 A）

> **状态**：**已采纳（2026-08-08）**；**2026-08-11 修订** — 官方终端为 Client 远程 `crabmate-tui`；本仓同进程 `chat|repl|tui` **命令入口已移除（D2.1）**；**实现硬删已完成（D2.2）**。  
> **执行清单**：[`client_shell_split_todo.md`](./client_shell_split_todo.md)  
> **契约发版（Phase 1）**：[`client_contract_versioning.md`](./client_contract_versioning.md)  
> **运行时 UI/API 拆分**：[`client_ui_runtime_split.md`](./client_ui_runtime_split.md)（`serve` 默认纯 API）  
> **远程 CLI/TUI（Client）**：同级 [`../crabmate-client/docs/design/remote_cli_tui.md`](../../../crabmate-client/docs/design/remote_cli_tui.md)（GitHub：[remote_cli_tui.md](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/remote_cli_tui.md)）  
> **关联**：[`client_turn_smoke_runbook.md`](./client_turn_smoke_runbook.md)、[`crate_dep_policy.md`](./crate_dep_policy.md)、[`web_host_extract.md`](./web_host_extract.md)、[`turn_runtime_placement.md`](./turn_runtime_placement.md)、[`web_host_p5_placement.md`](./web_host_p5_placement.md)；契约见 **`docs/SSE协议.md`**、**`docs/命令行与路由.md`**、**`docs/配置说明.md`**。  
> **非目标**：拆 Agent 微服务；另开第二套会话 API；把同进程 `run_agent_turn` 剪贴进 Client；多租户账号体系；以本 ADR 替代 turn-runtime / queue 搬家决策。  
> **本轮**：D2.2 已硬删 `runtime/tui`、同进程对话 REPL/`chat`、Cargo feature `repl`/`tui`（及 **reedline** / **ratatui**）；默认 features 为 **`web` + `mcp`**。

---

## 1. 背景

CrabMate **执行权威**在 **`crabmate serve`**（Agent / 工具 / 工作区）。桌面 / 移动壳已**不** spawn sidecar；业务 UI 与官方终端已在 Client，经 API 基址 + CORS / HTTP+SSE 连接 `serve`。模型密钥由 **Client 本机**存放，经请求体 **`client_llm.api_key`** 注入（**不**以本仓进程 `/api-key` 为官方路径）。

同进程 **`repl` / `tui` / `chat`** 实现与 clap 入口均已硬删（D2.2）；官方终端仅 Client **`crabmate-tui`**（HTTP + SSE）。

目标句：**本仓只维护 Server**；官方 Client（Desktop Linux、Android、浏览器直连、**远程终端**）可独立发版，只认稳定 HTTP/SSE。

---

## 2. 决策

### 2.1 路径 A（采纳）

| 项 | 决定 |
|----|------|
| 业务 UI | 由 Client / 可选独立 UI 仓构建与发版；经可配置 **API 基址** 连接兼容的 `serve` |
| 路径 B（UI 永远随 server 托管） | **不作终点**；不得用「UI 随 server」宣称「只维护 server」已完成（勿与下文「远程终端路径 B」混淆） |
| 本仓终点 | `serve` + 契约 crate + 运维子命令（`doctor` / `web-bearer` / `config` 等）；**官方终端不在本仓** |
| 同进程 `chat` / `repl` / `tui` | **已硬删（D2.2）**；官方终端为 Client **`crabmate-tui`** |
| 拆壳仓门槛 | **须先**完成契约可发布 + 前端 API 基址/CORS（见 todo Phase 1–2）；禁止跳过 Phase 2 |

### 2.2 官方 Client 矩阵

| 入口 | 形态 | 备注 |
|------|------|------|
| **Desktop Linux** | Tauri 壳（**`../crabmate-client/desktop-tauri/`**） | 回环可保留有限 IPC；非回环 IPC 降级可接受 |
| **Android** | Tauri 壳（**`../crabmate-client/mobile-tauri/`**） | 不 spawn 本机 Agent |
| **浏览器直连** | 静态托管官方 WASM/UI | 与壳共用同一套 UI 产物或同源构建 |
| **Terminal（远程）** | Client 仓 **`crabmate-tui`**（HTTP + SSE） | 与壳同契约钉 / Web Bearer / `client_llm`；**不**内嵌、**不** spawn `serve`；分期见 Client [`remote_cli_tui.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/remote_cli_tui.md) |

**不在官方矩阵**（可存在但不承诺一等公民）：macOS/Windows 桌面、iOS、IDE 扩展、IM 桥等——另开产品切片，仍只认同一契约。

**本仓同进程 `chat` / `repl` / `tui`**：**已硬删**（D2.1 入口 + D2.2 实现）；**不是**上表入口。无子命令时 clap 报错并提示使用 `serve` / 运维命令或 Client **`crabmate-tui`**。

### 2.3 远程终端（Client 路径 B，已拍板）

与「UI 永远随 server」的路径 B **无关**。Client 侧决定：官方终端为**远程客户端**（同 Tauri 壳），执行权威仅 `serve`。

| 项 | 决定 |
|----|------|
| 二进制 | **`crabmate-tui`**（避免与本仓 `crabmate` / Desktop `crabmate-desktop` 冲突） |
| 契约 | 钉 `crabmate-sse-protocol` / `crabmate-api-contract` 等（同 `frontend`） |
| 模型密钥 | **Client 本机**钥匙串 / Keystore → 请求体 **`client_llm.api_key`**（同 WASM UI） |
| 同进程 Agent CLI | **不**迁入 Client；本仓已软弃用 → **硬删（D2.2）** |

细节、分期（P0–P5）与仓内布局以 Client **`remote_cli_tui.md`** 为准；本 ADR 只钉**产品边界**。

### 2.4 密钥边界（不变）

| 密钥 | 用途 | 禁止 |
|------|------|------|
| **Web API 共享密钥**（`CM_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`；Client 连接页/侧栏/钥匙串） | 保护 HTTP API（`Authorization: Bearer` / `X-API-Key`） | 不得当作模型密钥发给上游 LLM |
| **模型密钥 / `client_llm.api_key`** | 上游 `chat/completions` 等；**权威存放在 Client** | 不得要求浏览器把模型密钥当 Web Bearer；日志/错误体不得打印完整值 |
| 进程环境 **`API_KEY`** | **可选回退**（无头 `serve`、旧脚本、`models`/`probe`/`e2e`/`bench` 等）；**不是**官方 Client 对话路径；服务端模型钥匙串槽已退役 | 不与 Web Bearer 混用 |
| 其它（GitHub、MCP bearer 等） | 各能力专用 | 不与上列混用 |

跨 Origin 仍只靠 **Web Bearer** + CORS 白名单；connect hash（`#cm_web_api_bearer=`）仅传递 Web API 密钥；非回环监听策略与现文档一致。

**硬删同进程对话后**：服务端 **`PUT /user-data/secrets/client-llm` / `executor-llm` 与模型钥匙串槽已退役**（Client 持钥）；进程 **`API_KEY`** 仍可为 `models`/`probe`/可选回退。**不得**删请求体 `client_llm` 消费路径。

### 2.5 弃用与删除分期（本仓）

| 阶段 | 内容 | 状态 |
|------|------|------|
| **D0** | ADR / 兼容矩阵写明官方停用 | ✅ |
| **D1** | clap help、启动 stderr、`命令行与路由` / README 指向 `crabmate-tui` | ✅ |
| **D2.1** | **移除** clap / 调度入口（`chat|repl|tui`）；须显式子命令；legacy 不再插 repl/chat | ✅ |
| **D2.2** | 硬删 `runtime/tui` 与同进程对话 REPL/`chat` 实现、Cargo feature `repl`/`tui`、`TuiLlmStreamScratch` / `TuiApprovalRequest` 链 | ✅ |
| **D3** | 收紧进程 `API_KEY`（可选）；man / CI / 冒烟清单 | 进行中（模型钥匙串槽已退役） |

---

## 3. 选项对比

| 选项 | 做法 | 收益 | 代价 |
|------|------|------|------|
| **A. Client 自带 UI（采纳）** | API 基址 + CORS；UI 随 Client/UI 仓发版；本仓可去 `frontend/` 源码 | 真正「只维护 server」；多端 Client 同契约可独立发版 | 跨 Origin、发版矩阵、双仓 CI 更复杂 |
| B. UI 随 Server | 壳只连 URL；UI 始终由 `serve` 托管 dist | 改动小；同 Origin 简单 | 本仓仍养 `frontend/`；发版边界未断 |
| C. 仅拆壳仓、UI 仍同源 | 先迁 Tauri 目录 | 主仓 CI 可甩掉 GTK | 易偷换成「假完成」；与目标句冲突 |

**终端面**：

| 选项 | 做法 | 本仓立场 |
|------|------|----------|
| **远程 `crabmate-tui`（采纳）** | Client 仓 HTTP/SSE 客户端 | **唯一**官方终端入口 |
| 同进程 `repl`/`tui` 迁入 Client | 剪贴 `run_agent_turn` / 工具栈 | **拒绝** |
| 软弃用 → 硬删同进程入口 | help/警告后删除代码 | **采纳并完成**（D1→D2.2） |

---

## 4. 后果与约束

1. **契约优先**：不新开会话 API；Client（含 `crabmate-tui`）只认现有 HTTP + SSE（见 `docs/SSE协议.md`）。
2. **与宿主解耦正交**：`turn_runtime_placement` / `web_host_p5_placement` **不阻塞**本决策。
3. **现状**：壳与业务 UI 在 **`../crabmate-client`**；远程终端见 Client `remote_cli_tui.md`（P2+）。兼容表见 [`client_compat_matrix.md`](./client_compat_matrix.md)。
4. **安全**：CORS 默认保守；非回环须 Bearer；模型密钥权威在 Client。
5. **删除纪律**：D2 硬删须独立 PR（或明确范围的提交），配套测试 / 冒烟清单 / man 再生；**禁止**在未完成 D1 文案前静默删入口。

---

## 5. 成功标准

**壳仓与远程终端只认已发布的协议版本与 API 基址；主仓只发布 `serve`（与契约）及运维 CLI——路径 A 为唯一终点。同进程 `chat|repl|tui` 不计入完成条件；D2 硬删完成后路径 A 终端面 closure。**

分阶段验收：[`client_shell_split_todo.md`](./client_shell_split_todo.md)；远程终端：Client **`remote_cli_tui.md`**；本仓弃用：上文 **§2.5**。
