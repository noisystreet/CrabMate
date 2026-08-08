# 三端真实回合冒烟（B3 Runbook）

> **状态**：清单已入库（2026-08-08）；**执行靠人工勾选**，默认不进 CI。  
> **关联**：[`turn_host_decouple.md`](./turn_host_decouple.md) 验收项；官方 Client 拆分见 [`client_shell_split.md`](./client_shell_split.md) / [`client_shell_split_todo.md`](./client_shell_split_todo.md)；自动化真 LLM 见 [`docs/真实LLM-E2E.md`](../真实LLM-E2E.md)。  
> **非目标**：改 SSE/HTTP 字段；把本清单做成强制 CI 门禁；拆 Desktop/Mobile 仓库。

---

## 1. 目的

在**不依赖模块搬家**的前提下，用最短路径确认：

1. **宿主面**：CLI / TUI / Web 都能经共享编排完成至少一轮真实（或等价）回合。  
2. **Client 面**：浏览器 / Desktop 壳 / Mobile 壳都能只凭 **URL + Bearer** 走 HTTP/SSE 完成一轮对话。  
3. **契约**：协议版本故意错位时失败可预期（`SSE_CLIENT_TOO_NEW` 等）。

发版前、合并影响 `TurnRunner` / SSE / connect 的大改后，建议跑一遍 **§4 最小集**；全量矩阵按需。

---

## 2. 前置（共用）

| 项 | 说明 |
|----|------|
| 密钥 | 默认 `llm_http_auth_mode=bearer` 时须有可用 **`API_KEY`**（或钥匙串 / 侧栏已存）；**勿**把真密钥写进本文件或 commit |
| 前端静态包 | Web / Desktop / Mobile 远程 UI：`cd frontend && trunk build`，再启动或重启 `serve` |
| 工作区 | 选一可信本地目录作 `--workspace` / Web 当前工作区 |
| 代理 | Playwright / 本机 `127.0.0.1` 时注意 `no_proxy=127.0.0.1,localhost`（见 `AGENTS.md`） |
| Bearer | 若启用 Web API 共享密钥：侧栏 / 连接页填的是 **`CM_WEB_API_BEARER_TOKEN`**，**不是**模型 `API_KEY` |

**提示词（各端统一）**：用户消息发 `用一句话介绍你自己`（或等价短问候）。**通过**：收到助手终答或流式结束，无未处理错误弹层。

---

## 3. 与自动化测试的关系

| 层级 | 文档 / 命令 | 能否替代本 runbook |
|------|-------------|-------------------|
| 编排真 LLM | `crabmate e2e` / `REAL_LLM_E2E=1`（见 [`真实LLM-E2E.md`](../真实LLM-E2E.md)） | **部分替代** CLI 宿主面；**不**覆盖 TUI UI / 壳连接页 |
| HTTP SSE 真 LLM | `REAL_LLM_E2E=1 cargo test e2e_http_` | **部分替代** Web 协议路径；**不**覆盖浏览器壳 |
| Playwright mock | `e2e/specs/mock-*.spec.ts` | **不**替代（无真模型 / 无壳生命周期） |
| Victauri 真 LLM | Client 仓 `../crabmate-client` + `REAL_LLM_E2E=1`；`./scripts/victauri-e2e.sh` 另起 `serve` | **可选替代** Desktop **薄壳 + 本机 serve** 路径 |

本 runbook 的价值是：**跨入口人工勾选 + 协议错位 + 远程壳**，补自动化盲区。

---

## 4. 最小集（发版 / 大改后优先）

按顺序勾选；任一项失败先记现象再继续（勿跳过协议错位）。

### 4.1 宿主：CLI

```bash
# 仓库根；工作区与密钥按本机调整
API_KEY='…' cargo run -- --workspace /path/to/ws chat -- "用一句话介绍你自己"
```

- [ ] 退出码 0（或文档约定的成功路径），stdout/日志可见助手回复摘要  
- [ ] 无密钥明文打进日志

### 4.2 宿主：Web（浏览器本机）

```bash
cd frontend && trunk build && cd ..
API_KEY='…' cargo run -- --workspace /path/to/ws serve --host 127.0.0.1
# 浏览器打开打印的 URL；若配置了 Web Bearer，侧栏先保存同一共享密钥
```

- [ ] 发送 §2 提示词，流式气泡完成，无卡死  
- [ ] （可选）响应或错误 JSON 含 `request_id` / 响应头 `x-request-id`（契约卫生）

### 4.3 契约：SSE 客户端过新

对已启动的 `serve`（替换 URL / Bearer）：

```bash
# 注意：字段是 message（不是 messages）；本机若走 Privoxy 须 no_proxy=127.0.0.1,localhost
curl -sS -o /tmp/cm_sse_too_new.json -w '%{http_code}\n' \
  -X POST 'http://127.0.0.1:8080/chat/stream' \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer YOUR_WEB_API_BEARER' \
  -d '{"message":"hi","client_sse_protocol":99}'
```

- [ ] HTTP **400**，体中 `code` 为 **`SSE_CLIENT_TOO_NEW`**（`v99`）；体与头均有同值 **`request_id` / `x-request-id`**  
- [ ] （可选）`client_sse_protocol: 0` → **`INVALID_SSE_CLIENT_PROTOCOL`**；低于服务端的正整数 → **`SSE_PROTOCOL_MISMATCH`**  
- [ ] 未配置 Bearer 且服务端亦无密钥时，可去掉 `Authorization`；若服务端要求 Bearer 则先配对再测

### 4.4 Client：Desktop（薄壳 + 本机已启动的 serve）

**权威步骤在 Client 仓**：同级 [`../crabmate-client/docs/design/shell_smoke_runbook.md`](../../../crabmate-client/docs/design/shell_smoke_runbook.md) §2（过渡期主仓仍有 `desktop-tauri/` 副本时亦可按其 README）。

壳**不再** spawn `serve`；先起后端，再开** Client 仓**桌面壳（或用 `CM_DESKTOP_SERVE_URL` 跳过连接页）。

```bash
# 终端 A：本仓 serve
cargo run -- serve --host 127.0.0.1 --port 8080
# 终端 B：Client 仓
cd ../crabmate-client/desktop-tauri/src-tauri
cargo tauri dev
```

- [ ] 闪屏 → 连接页预填本机 URL → 探测成功进入 UI  
- [ ] 完成一轮对话（同 §2 提示词）  
- [ ] （可选）连接页改填 LAN 上另一台 `serve`，能聊且远程非回环时桌面 IPC 受限符合预期

### 4.5 Client：Mobile 或「桌面当远程壳」

见 Client 仓 [`shell_smoke_runbook.md`](../../../crabmate-client/docs/design/shell_smoke_runbook.md) §3。

任选其一：

**A. Android APK**（`../crabmate-client/mobile-tauri`）：连接页填 `http://<LAN-IP>:8080/` + 与服务器相同的 Web Bearer → 一轮对话。

**B. 无真机时**：用 Desktop 连接页指向**另一**已启动的 `serve`（或本机第二端口），等同验证「薄壳 + 远程权威」。

- [ ] 连接成功，hash Bearer 交接后能发消息  
- [ ] 侧栏「断开」/ `?manual=1` 行为符合 README（不立刻误重连）

---

## 5. 全量矩阵（按需）

### 5.1 宿主面（TurnRunner / 同进程）

| ID | 入口 | 命令或步骤 | 通过标准 | 勾选 |
|----|------|------------|----------|------|
| H1 | CLI `chat` | §4.1 | 终答可见 | [ ] |
| H2 | CLI `repl` | `cargo run -- --workspace … repl`，发一条用户消息后退出 | 同左 | [ ] |
| H3 | TUI | `cargo run -- --workspace … tui`（须 TTY），发一条 | 中区出现助手输出 | [ ] |
| H4 | Web UI | §4.2 | 流式完成 | [ ] |
| H5 | Web 队列（可选） | UI 或 `POST /chat/async` 后查 job | job 终态成功 | [ ] |

编排级自动化可记：`crabmate e2e` 中 `orch_single_agent_smoke` 绿 → 在备注栏写「H1≈e2e」。

### 5.2 Client 面（仅 HTTP/SSE）

| ID | 入口 | 步骤 | 通过标准 | 勾选 |
|----|------|------|----------|------|
| C1 | 浏览器 → 本机 serve | §4.2 | 一轮对话 | [ ] |
| C2 | 浏览器 → LAN/VPS serve | 同左，换 URL + Bearer | 一轮对话 | [ ] |
| C3 | Desktop 薄壳 → 本机 serve | §4.4 | 一轮对话 | [ ] |
| C4 | Desktop → 远程 serve | 连接页改远程 URL | 一轮对话；IPC 预期 | [ ] |
| C5 | Mobile → 远程 serve | §4.5 A | 一轮对话 | [ ] |

### 5.3 契约负面

| ID | 场景 | 期望 | 勾选 |
|----|------|------|------|
| P1 | `client_sse_protocol` 过大 | §4.3 → `SSE_CLIENT_TOO_NEW` + `request_id` | [ ] |
| P2 | （可选）旧前端连新 server | UI 出现 `SSE_SERVER_TOO_NEW` 类失败，无半解析卡死 | [ ] |
| P3 | （可选）错误响应 | 有 `request_id` 或 `x-request-id`，**无**完整密钥 | [ ] |

---

## 6. 执行记录（复制填写）

```text
日期：
执行人：
分支 / commit：
serve 绑定：127.0.0.1 / 0.0.0.0 / VPS
最小集 §4：H1 / Web / P1 / Desktop / Mobile-or-remote = pass|fail
失败摘要（码 / 截图路径，勿贴密钥）：
备注（是否用 crabmate e2e / Victauri 替代某项）：
```

记录可留在本地或 `agent_space/`（示例文件名：`b3-smoke-execution-YYYY-MM-DD.md`）；**不要**把含密钥的日志提交进仓库。  
仓库内**不**强制勾选 §4——以本地/agent_space 执行记录为准；全最小集绿后再勾 [`turn_host_decouple.md`](./turn_host_decouple.md)「各端一次真实回合」。

---

## 7. 何时算 B3「清单完成」vs「冒烟通过」

| 状态 | 含义 |
|------|------|
| **清单完成** | 本文存在且被 `turn_host_decouple` / 开发文档索引；B3 冒烟清单已交付 |
| **冒烟通过** | 某人按 §4（或 §5）勾选并留下 §6 记录；可将 `turn_host_decouple` 中「各端一次真实回合」改为已勾 |

后续若要把最小集自动化：优先扩展现有 `e2e_http_` / `crabmate e2e`，而不是把人工壳步骤硬塞进 pre-commit。

---

## 8. 相关路径

| 路径 | 角色 |
|------|------|
| `src/turn_runner.rs` | Web 队列注入面 |
| `src/runtime/cli/` | CLI / repl / chat |
| `src/runtime/tui/` | TUI |
| `crates/crabmate-connect/` | （过渡）本仓副本；**权威在** `../crabmate-client/crates/crabmate-connect` |
| `desktop-tauri/`、`mobile-tauri/` | （过渡）本仓副本；**权威在** `../crabmate-client` |
| `../crabmate-client/docs/design/shell_smoke_runbook.md` | 壳人工冒烟 |
| `../crabmate-client/docs/TESTING.md` | Victauri / Client pre-commit |
| `docs/SSE协议.md`、`docs/命令行与路由.md` | 契约真源 |
| `docs/真实LLM-E2E.md`、`docs/测试指南.md` | 自动化入口（Server / Playwright） |
| `docs/design/client_contract_versioning.md` | 契约 semver / git tag |
| `docs/配置说明.md` | `CM_WEB_CORS_ALLOWED_ORIGINS` / API 基址（浏览器） |

---

## 9. 可选：跨 Origin 浏览器直连（路径 A Phase 2）

静态托管官方 UI（或任意 Origin 的 `frontend/dist`），API 指向另一 Origin 的 `serve`。

### 9.1 服务端

```bash
cd frontend && trunk build && cd ..
# 将 UI Origin 加入白名单（示例：静态站在 :8081，API 在 :8080）
CM_WEB_API_BEARER_TOKEN='…shared…' \
CM_WEB_CORS_ALLOWED_ORIGINS='http://127.0.0.1:8081' \
API_KEY='…' cargo run -- --workspace /path/to/ws serve --host 127.0.0.1 --port 8080
```

- CORS **默认关闭**（空白名单）；非空为**精确 Origin**（无 `*`）；改白名单须**重启 serve**。
- 跨 Origin 仍只认 **Web Bearer**（不是模型 `API_KEY`）。
- 启用 CORS 时服务端会 **expose** **`x-conversation-id` / `x-stream-job-id` / `x-request-id`**（否则浏览器 JS 读不到，会话绑定与断线重连会 silently 失效）。

### 9.2 浏览器（静态 UI）

另开静态托管（示例）：

```bash
# 任意静态服务器指向 frontend/dist；Origin 须在上表白名单内
python3 -m http.server 8081 --directory frontend/dist
```

打开 `http://127.0.0.1:8081/` → **设置**：

1. **Web API 共享密钥** = 与 `CM_WEB_API_BEARER_TOKEN` 相同  
2. **API 基址** = `http://127.0.0.1:8080`（留空则同 Origin）  
3. 保存后发送 §2 提示词  

- [ ] 一轮对话流式完成  
- [ ] **同会话第二轮**：再发一句短消息，确认仍落在同一 `conversation_id`（DevTools → Network → `/chat/stream` 响应头，JS 可读 `x-conversation-id` / `x-stream-job-id`）  
- [ ] 故意错位 API 基址或 Bearer 时失败可预期（非模糊 500）  

**对照**：§4.2 同 Origin（`serve` 托管 dist、API 基址留空）行为不变。
