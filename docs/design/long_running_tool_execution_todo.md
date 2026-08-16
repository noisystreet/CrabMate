# 长耗时工具执行：待办

**状态**：设计待办（未承诺排期）。**受众**：维护 `tool_registry`、`run_command`、`execute_tools`、SSE 工具事件的开发者。  
**语言**：中文。  
**跟踪**：落地后从 **`docs/待办清单.md`**（`tools/` 章）删除对应条目；本文件可改为修订记录或删节。

**关联**：

- 工具契约 / 信封：`docs/工具说明.md`
- 超时与 `[tool_registry]`：`docs/配置说明.md`
- SSE `tool_output_chunk` / `error_code`：`docs/SSE协议.md`
- 工具调用演进（含「长任务进度事件」一行）：`docs/design/tool_calling_evolution.md`
- 安全面：`.cursor/rules/security-sensitive-surface.mdc`（`run_command` 白名单、路径、审批）

---

## 目标与非目标

**目标**（按优先级）：超时与用户取消能真正终止子进程；长命令执行中 UI 可见进度；超时结果带部分输出，模型能据此重试或缩小范围；超时按工具类分层，而不是全局放大 `command_timeout_secs`。

**非目标**：

- 把默认 `command_timeout_secs`（当前 600）再加大作为「解决方案」。
- 写工具并行（仍走串行批）。
- 把完整构建日志塞进模型上下文（继续信封 `summary` + 截断；细节走 UI chunk）。
- 用 Docker 沙盒超时替代进程级取消（沙盒是隔离，不是杀进程）。
- 第一阶段不做「后台 job + 轮询」新契约（见 §4）。

---

## 现状与痛点

| 现象 | 代码落点 |
|------|----------|
| 多数工具：`spawn_blocking` + 外圈 `tokio::time::timeout` | `src/cm_internal/tool_registry/execute/execute_run_command.inc.rs`、`execute_dispatch_body.inc.rs`、`execute_http_tools.inc.rs` |
| `run_command` 用 `Command::output()` 等整段结束 | `src/cm_tools/tools/command.rs` `run_impl` |
| 外圈超时**不取消** blocking 任务；workflow 已注明可能孤儿进程 | `src/cm_workflow/execute/node.rs` |
| SSE 断开 / 取消只在**工具之间**检查 | `src/agent/agent_turn/host/execute/tools/serial/exec_serial.rs` |
| 仅 `terminal_session` 流式 `tool_output_chunk` | `src/cm_internal/terminal_session/`、`execute_terminal_session.inc.rs` |
| `python_snippet_run` 已有墙钟 + `SIGKILL`（对照实现） | `src/cm_tools/tools/python_tools.rs` |
| 并行只读批已有按 `ToolExecutionClass` 的墙钟覆盖 | `[tool_registry].parallel_wall_timeout_secs`；串行写工具仍共用 `command_timeout_secs` |

HTTP SSE 有 `KeepAlive`，但工具气泡在 `run_command` 结束前通常无增量，体验仍像卡住。

---

## 0. 现有能力（实现前可先用）

不必等代码：交互/长构建优先 **`terminal_session`**（chunk + `send_signal` / `close`）；短命令保持较小全局超时；`python_snippet_run` 可用 `timeout_secs`（1～600）；只读工具走并行批；`cargo test` 可命中 `test_result_cache`；反代须足够大的 `proxy_read_timeout`（见 `docs/个人VPS部署指南.md`）。

---

## 1. P0 — 超时与取消必须杀掉子进程

**问题**：外圈超时只放弃等待 JoinHandle；`run_command` 子进程继续跑。关页面同样杀不掉当前工具。

**待实现**：

- [ ] **`run_command`（及同类子进程工具）管 `Child`**：进程组；超时 **SIGTERM → 短等待 → SIGKILL**。对照 `python_snippet_hard_kill`。
- [ ] **外圈 `tokio::time::timeout` 触发后走同一套杀进程**，不只返回「命令执行超时（N 秒）」。
- [ ] **用户取消 / SSE 关闭**穿进 Child wait（已有 `TurnControlSink` / `AtomicBool cancel`），不要只在工具间隙 `abort_tool_batch_if_sse_closed`。
- [ ] **workflow 节点超时**与 `run_command` 共用杀进程语义，去掉「请手动检查孤儿进程」作为唯一出路。

**建议实现形态**：子进程类改 `tokio::process::Command`（或 blocking 内 `try_wait` 轮询）+ `kill_on_drop`；不要把长子进程堆在 `spawn_blocking` 池里占死线程。

**关联文件**：`command.rs`、`execute_run_command.inc.rs`、`exec_serial.rs`、`cm_workflow/execute/node.rs`、`python_tools.rs`（对照）。

**测试**：超时后进程不存在（可用短 `sleep` 夹具）；取消标志置位后 Child 退出；workflow 节点超时同样杀进程。勿在测试里残留 sleep 进程。

**文档**：`docs/工具说明.md`（超时=杀进程）、`docs/配置说明.md`；workflow 超时说明。

**安全**：杀进程组勿误伤 serve 自身；白名单 / 路径 / 审批规则不得因改 wait 路径被绕过。

---

## 2. P1 — 流式输出 + 超时带部分结果

**问题**：长命令对用户和模型都是黑盒；超时信封几乎没有已产生的 stdout/stderr。

**待实现**：

- [ ] **`run_command` / 测试类工具**（`cargo test`、`pytest_run` 等）按行或块下发已有 SSE **`tool_output_chunk`**（`tool_call_id`、`seq`、可选 `stream: stdout|stderr|combined`）。**chunk 不进模型上下文**（见 `docs/SSE协议.md`）。
- [ ] **超时 / 取消**时把已截断输出写入 `tool_result` 信封，稳定 **`error_code: timeout`**（或取消码）与现有 `retryable` 启发式对齐。
- [ ] **长时间无输出 heartbeat**（已耗时、仍在跑），避免 UI / 中间代理当连接死亡。优先复用现有控制面事件或明确的 debug/进度 payload；**新顶层 SSE 键**须同步 Client `parser_v2.rs`、`sse_dispatch/types.rs`、`src/cm_sse_protocol/control_classify.rs`、金样（见 `.cursor/rules/api-sse-chat-protocol.mdc`）。

**建议**：第一刀可只做超时附带部分输出（不改 SSE 形状）；第二刀再接 `tool_output_chunk`（协议已有，Client 已 `onToolOutputChunk`）。

**关联文件**：`command.rs`、`emit.rs`、`cm_sse_protocol/sse/protocol.rs`、Client `frontend/src/api/chat_stream/parser_v2.rs`。

**测试**：chunk `seq` 单调；超时信封含部分 stdout；金样若改控制面分发则跑本仓 `golden_ag_ui_classify_matches_expected` 与 Client `golden_ag_ui_v2_parser_matches_expected`。

---

## 3. P2 — 按工具类分层墙钟

**问题**：`ls` 与全量测试共用 600s；串行写路径缺少与并行批对等的按类覆盖。

**待实现**：

- [ ] 默认分层（数值可在实现时用配置钉死并写进 `config/tools.toml` 注释）：瞬时读/列目录 10–30s；HTTP 保持现有内外圈；构建/测试可配置且允许单次 **`timeout_secs` 覆盖**（钳制上限，对齐 `python_snippet_run` 1～600 模式）；`terminal_session` 跟会话生命周期，不占用 turn 墙钟。
- [ ] 串行 `dispatch_tool` 与 `[tool_registry].parallel_wall_timeout_secs` 叙事统一（按 `ToolExecutionClass` 或工具名）。
- [ ] **禁止**把「个别慢命令」解决成全局超时到数十分钟。

**关联文件**：`config/tools.toml`、`cm_config/`、`cm_tools/registry_policy.rs`、`tool_dispatch.rs`。

**文档**：`docs/配置说明.md` 表；`README.md` 若出现使用者可见的新键 / `CM_*`。

---

## 4. P3 — 执行模型与观测（可后做）

- [ ] **子进程类**走 async `Child` + 管道；**短同步 SDK / 文件 IO** 仍 `spawn_blocking`，墙钟宜短或内部可中断。
- [ ] **观测**：工具时长直方图、超时率、是否已 kill、残留 Child 计数；日志已脱敏（`.cursor/rules/secrets-and-logging.mdc`）。当前超时多半一条 `error!`，排障看不到杀没杀掉。
- [ ] **后台 job + `job_id` 轮询/订阅**（可选产品能力）：不绑死当前 LLM turn。须单独设计契约与兼容窗口；**不要**与 P0/P1 绑在同一 PR。

与路线图「可观测与执行轨迹」、`tool_calling_evolution.md`「长任务进度事件」同向，落地时合并拆解，避免三处各写一套需求。

---

## 建议落地顺序（PR 切片）

1. **`run_command` 超时/取消杀进程 + 超时附带已捕获输出**（行为变化，补测试；可不改 SSE 顶层键）。
2. **`run_command` / 测试工具复用 `tool_output_chunk`**（协议已有；Client 展示对齐）。
3. **按工具类默认超时 + 可选 `timeout_secs`**。
4. 观测指标；若产品需要再开后台 job ADR。

对照实现：`python_snippet_run`（墙钟 + kill）、`terminal_session`（chunk + 信号）。

---

## 完成定义（单条可删待办时）

- 超时或取消后，夹具子进程不残留。
- 超时 `tool_result` 含截断部分输出与稳定 `error_code`。
- 长 `run_command` 在 Client 工具气泡可见增量（若已做 P1 chunk）。
- `docs/工具说明.md` / `docs/配置说明.md`（及若改 SSE：`docs/SSE协议.md` + Client parser）已同步。
- 白名单、路径、审批行为有回归测试，未弱化。
