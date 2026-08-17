# 长耗时工具执行：待办

**状态**：P0 已落地；P1 宿主 `run_command` 的 `tool_output_chunk` 已落地；**`python_snippet_run` 已迁入共享会话（删除 pid-only hard_kill）**；**P3 观测已落地**（会话时长直方图、超时/取消/已 kill/残留计数 + 脱敏日志）；测试类工具 chunk 与 P2–P3 其余项未承诺排期。**受众**：维护 `tool_registry`、`run_command`、`execute_tools`、SSE 工具事件的开发者。  
**语言**：中文。  
**跟踪**：落地后从 **`docs/待办清单.md`**（`tools/` 章）删除对应条目；本文件可改为修订记录或删节。

**关联**：

- 工具契约 / 信封：`docs/工具说明.md`
- 超时与 `[tool_registry]`：`docs/配置说明.md`
- SSE `tool_output_chunk` / `error_code`：`docs/SSE协议.md`
- 工具调用演进（含「长任务进度事件」一行）：`docs/design/tool_calling_evolution.md`
- 安全面：`.cursor/rules/security-sensitive-surface.mdc`（`run_command` 白名单、路径、审批）

---

## 已拍板（审查后）

1. **第一刀只覆盖宿主 `run_command`**（含审批 `skip_arg_safety` 路径）以及 **workflow 节点里对 `run_command` 的超时**。`cargo_test` / `pytest_run` / 各 `run_and_format*` / `python_snippet_run` / 动态工具 / MCP **不**算 P0 完成定义，但必须抽出可复用会话，避免每个工具再抄一套 wait。
2. **部分输出与杀进程同一套 IO**：不能继续 `Command::output()`。第一刀就要 `spawn` + **并发排空** stdout/stderr（防管道死锁），截断仍走 `command_max_output_len`。P1 只是把已有缓冲 `emit` 成 `tool_output_chunk`。
3. **`python_snippet_hard_kill` 是反例，不是模板**：只对直接 pid `SIGKILL`、无进程组、超时无部分输出。共享实现要升级（见 §共享原语）。
4. **禁止**用现有 `ToolExecutionClass` 区分「`ls` vs 全量测试」。`run_command` 与 `terminal_session` 同属 `CommandSpawnTimeout`；`cargo_test` 等走 `SyncDefault` / `BlockingSync`。分层超时按 **工具名配置 + 调用方 `timeout_secs`**（见 P2）。
5. **Docker 沙盒**（`dispatch_non_sync_tool_to_docker` / `run_tool_in_docker`）：P0 **不**承诺杀容器内进程；超时只保证宿主 wait 有界。沙盒 reap 单独立项（P3 或单独 ADR）。
6. **Unix 优先**（`setsid`/`setpgid` + SIGTERM→SIGKILL）。Windows 可后做；若做，对齐现有 snippet 的 `taskkill /T`，不要假装与 Linux 进程组等价。
7. **代理保活 ≠ 工具进度**：`chat_handlers/chat/stream.rs` 已有 axum `KeepAlive`。P1 **默认不新增顶层 SSE 键**；UI 进度靠 `tool_output_chunk`。无输出心跳仅当产品确认气泡必须显示「已耗时」时再加，并走协议清单。

---

## 目标与非目标

**质量属性（冲突时按此序）**：进程不泄漏（安全/可靠性）→ 超时/取消对模型有部分输出（正确性）→ UI 可见增量（体验）→ 按工具名可配墙钟（可运维）→ 观测指标。

**目标**：

- 超时与用户取消能真正终止 **本刀范围内** 的子进程（含 `bash -c` 孙进程所在进程组）。
- 超时/取消信封带截断后的已捕获输出，稳定 `error_code`（`timeout` / 取消码），`retryable` 与现启发式一致。
- 长 `run_command` 执行中 Client 工具气泡可追加 `tool_output_chunk`（P1）。
- 缩短「误用全局 600s」的路径：给 **具名工具** 和 **单次 `timeout_secs`** 配墙钟，而不是放大默认 `command_timeout_secs`。

**非目标**：

- 把默认 `command_timeout_secs`（当前 600）再加大作为解决方案。
- 写工具并行（仍走串行批）。
- 把完整构建日志塞进模型上下文（继续信封 `summary` + 截断；细节走 UI chunk）。
- 用 Docker 沙盒超时替代宿主进程级取消；P0 也不实现容器内 kill。
- 第一阶段不做「后台 job + 轮询」新契约（见 P3）。
- 不把 MCP / 动态工具 / 全量 `run_and_format` 纳入 P0 完成定义。
- 不按 `ToolExecutionClass` 给「列目录 10–30s、构建数分钟」做默认值（class 粒度不够）。

---

## 范围图

| 路径 | P0 | P1 chunk | P2 墙钟 | 说明 |
|------|----|----------|---------|------|
| 宿主 `run_command`（`command.rs` + `execute_run_command.inc.rs`） | 必做 | 必做 | `timeout_secs` 钳制 + 保持全局默认 | 含 `skip_arg_safety` |
| workflow 节点且 `tool=run_command` | 必做（共用杀进程） | 不做 SSE | 沿用节点 SLA / 全局默认 | 节点路径 **无** `TurnControlSink`；取消仅超时 |
| `cargo_test` / `pytest_run` / 其它 `run_and_format*` | 迁入共享原语即可，可下一 PR | 随后 | **按工具名** 键，不是 class | 仍是 `SyncDefault` + 外圈 `spawn_blocking` timeout |
| `python_snippet_run` | 迁入共享原语（替换 hard_kill） | 可选 | 已有 `timeout_secs` 1～600 | 外圈 blocking timeout 仍在 |
| `terminal_session` | 不改 wait 模型 | 已有 chunk | **保持**现有 turn 墙钟作安全网；超时须 `close`/`send_signal`，勿只丢 future | 见 P2 |
| Docker 沙盒内同类工具 | 不做 | 不做 | 不做 | 另立项 |
| MCP 代理工具 | 不做 | 不做 | 已有 `tool_timeout_secs` | 另立项 |

---

## 现状与痛点

| 现象 | 代码落点 |
|------|----------|
| 多数工具：`spawn_blocking` + 外圈 `tokio::time::timeout`，超时只丢 JoinHandle | `execute_run_command.inc.rs`、`execute_dispatch_body.inc.rs`、`execute_http_tools.inc.rs`、`dynamic_tools` 分发 |
| `run_command` 用 `Command::output()` 整段结束 | `src/cm_tools/tools/command.rs` `run_impl` |
| `cargo_test` / `pytest_run` / lizard / go / npm 等各自 `Command` + `run_and_format*` | `cargo_tools.rs`、`python_tools.rs`、… |
| workflow 超时注明孤儿进程 | `src/cm_workflow/execute/node.rs` |
| SSE 断开 / 取消只在**工具之间**检查 | `exec_serial.rs`、`abort_tool_batch_if_sse_closed`；`run_command` 看不到 `RunLoopIo.cancel` |
| 仅 `terminal_session` 流式 `tool_output_chunk` | `terminal_session/`、`execute_terminal_session.inc.rs`（**已有** `parallel_tool_wall_timeout_secs` 包一层） |
| `python_snippet_run`：墙钟循环 + **仅杀直接 pid** | `python_tools.rs`；**无**进程组、超时无部分 stdout |
| 并行只读批可按 `ToolExecutionClass` 覆盖墙钟 | `[tool_registry].parallel_wall_timeout_secs`；**不能**区分同一 class 内的短/长命令 |
| HTTP SSE `KeepAlive` 已有；工具气泡无增量仍像卡住 | `web/chat_handlers/chat/stream.rs` |
| 正文含「超时」已启发式 `error_code: timeout` | `cm_tools/tool_result/mod.rs`；缺口是空输出 + 杀不掉进程，不是缺码 |
| Docker 模式先 `run_tool_in_docker` | `dispatch_non_sync_tool_to_docker` |

---

## 0. 现有能力（实现前可先用）

不必等代码：交互/长构建优先 **`terminal_session`**（chunk + `send_signal` / `close`）；短命令保持较小全局超时；`python_snippet_run` 可用 `timeout_secs`（1～600）；只读工具走并行批；`cargo test` 可命中 `test_result_cache`；反代仍建议足够大的 `proxy_read_timeout`（见 `docs/个人VPS部署指南.md`）。

---

## 共享原语（P0 起必须落地，后续工具迁入）

建议模块位置（实现时可微调，保持 `cm_tools` 可被 workflow / registry 调用、不反向依赖 Web）：例如 `src/cm_tools/subprocess_session.rs`（名称不强制）。

**职责**（一个会话对象，禁止两套 wait）：

- `spawn`：stdin 关闭；stdout/stderr **piped**；Unix 上子进程 **新进程组/会话**（`setpgid(0,0)` 或 `setsid`），**禁止**对 serve 所在组发信号。
- **并发 drain** 两路管道（或等价的「读一边、另一边不堵死」），避免 stderr 填满导致子进程挂起、外圈超时更难回收。
- 累计输出遵守 `command_max_output_len`（及现有行数上限）；超限停止追加，仍要能 kill。
- 等待循环同时看：进程退出、墙钟、**可选** `Arc<AtomicBool>` 取消。chat 路径传入 `RunLoopIo.cancel` 与「SSE sender closed」；workflow **只传超时**。
- 超时/取消：**进程组 SIGTERM → 短等待 → 进程组 SIGKILL**；记录是否已 kill（供日志，脱敏、不打 argv 密钥）。
- 返回：exit / timeout / cancelled + 已截断 stdout/stderr。显式 `ToolError` 码（`timeout` / 取消），不要只靠正文含「超时」。
- 优先 `tokio::process::Command` + `kill_on_drop`，避免长命令占死 `spawn_blocking` 线程池。若短期仍在 blocking 线程里 `try_wait` 轮询，须在文档/PR 写明占用时长与迁出计划。

**迁入顺序**：`run_command`（✅）→ `python_snippet_run`（✅ 已迁入，pid-only `hard_kill` 已删除；新增进程组超时回归测试）→ 各 `run_and_format*`（未开始；P0 之后、P1 测试工具 chunk 之前或同时）。

**安全**：改 wait **不得**绕过白名单、`..`/绝对路径、审批门闩。超时/取消 **不得** 走 `is_compile_command_success` 成功路径，**不得** 写入 `test_result_cache`，**不得** 把 `workspace_changed` 打成 true。

---

## 1. P0 — 超时与取消必须杀掉子进程（+ 部分输出缓冲）

**问题**：外圈超时只放弃 JoinHandle；`run_command` 子进程继续跑。关页面同样杀不掉当前工具。无管道则无法带部分输出。

**待实现**：

- [x] 宿主 `run_command` 改用共享会话（含 `skip_arg_safety`）。
- [x] `execute_run_command.inc.rs` 外圈 `tokio::time::timeout` **触发后必须**对同一会话 kill，不能只返回「命令执行超时（N 秒）」而放任 Child。
- [x] chat：把 `cancel` / SSE closed **穿进** wait 循环，不要只在工具间隙 `abort_tool_batch_if_sse_closed`。
- [x] workflow：`tool=run_command` 的节点超时走同一套 kill；删掉「请手动检查孤儿进程」作为 **run_command** 的唯一出路。其它工具节点仍可孤儿，日志须写明范围。
- [x] **`run_tool_result` / `run_tool_dispatch` 必须走 `run_try_wait` + `ctx.command_timeout_secs`**，不能只用无墙钟的 `run_try`。workflow 跳过外圈 `tokio::time::timeout` 后，否则节点会无限挂起。
- [x] 超时/取消返回已截断输出 + 稳定 `error_code`（现启发式可保留作兜底）；`ToolError.legacy_parsed` 保留部分 stdout/stderr。
- [x] 杀进程后 **drain join / `try_wait` 失败路径须有界**（孤儿占管不得让 `spawn_blocking` 永久卡住）。

**关联文件**：`command.rs`、`execute_run_command.inc.rs`、`exec_serial.rs`（传 cancel）、`cm_workflow/execute/node.rs`、新建会话模块、`python_tools.rs`（迁入时对照删除 hard_kill）。

**测试**（勿残留 sleep）：

- 短 `sleep`：超时后 **pid 与进程组内子进程**均不存在（覆盖 `bash -c 'sleep …'` 孙进程，不只直接 Child）。
- `cancel` 置位后 Child 退出。
- workflow 节点对 `run_command` 超时同样杀组。
- 管道：子进程向 stderr 狂写时仍能在超时后退出（死锁回归）。
- 超时结果不缓存、不 `workspace_changed`。
- 白名单 / 不安全 argv / 审批回归（改 wait 不得放行原拒绝用例）。

**文档**：`docs/工具说明.md`（超时=杀进程组）；`docs/配置说明.md`；workflow 超时说明写清「目前仅 `run_command` 节点可 reap」。

---

## 2. P1 — 把已有缓冲打成 `tool_output_chunk`

**问题**：长命令对用户是黑盒。P0 已有内存中的增量缓冲，但未下发 SSE。

**待实现**：

- [x] **`run_command` 先**（宿主 chat 路径把捕获缓冲打成 `tool_output_chunk`）。
- [ ] 测试类工具（`cargo_test`、`pytest_run` 等）须已迁入共享会话后再 chunk。
- [x] 按行或块下发已有 **`tool_output_chunk`**（`tool_call_id`、`seq`、可选 `stream`）。**chunk 不进模型上下文**（`docs/SSE协议.md`）。总量仍受 `command_max_output_len` 约束，避免气泡无限涨。（workflow **不下发** chunk。）
- [x] **默认不加新顶层 SSE 键**。代理靠现有 HTTP `KeepAlive`。若必须做「无输出仍显示已耗时」，复用控制面/debug 形状，并同步 Client `parser_v2.rs`、`sse_dispatch/types.rs`、`control_classify.rs`、金样（`.cursor/rules/api-sse-chat-protocol.mdc`）。

**关联文件**：会话模块、`serial/emit.rs`、`cm_sse_protocol/sse/protocol.rs`、Client `parser_v2.rs`。

**测试**：chunk `seq` 单调；超时信封含部分 stdout；改控制面分发时跑本仓 `golden_ag_ui_classify_matches_expected` 与 Client `golden_ag_ui_v2_parser_matches_expected`。

---

## 3. P2 — 按工具名分层墙钟（不是按 class 一刀切）

**问题**：`ls`（经 `run_command`）与全量测试（`cargo_test` 或 `run_command cargo test`）不能靠 `ToolExecutionClass` 分开；给 `BlockingSync` 默认 10–30s 会误杀 snippet/测试。

**待实现**：

- [ ] **保持** `command_timeout_secs` 作为 `run_command` 与未点名工具的默认上限；**禁止**为迁就个别慢命令把全局默认调到数十分钟。
- [ ] 配置增加 **按工具名** 的墙钟覆盖（可与 `parallel_wall_timeout_secs` 并列，键为工具名而非 class）。建议先点名：`cargo_test`、`pytest_run`、以及确为瞬时的只读工具（若其外圈仍走 600s `spawn_blocking` timeout）。数值实现时钉进 `config/tools.toml` 注释。
- [ ] **`run_command` / `cargo_test` / `pytest_run`**：可选参数 **`timeout_secs`**，钳制上限（对齐 `python_snippet_run` 1～600 模式）。模型要跑长构建应显式传大值，而不是暗示「class=command 就是 30s」。
- [ ] **不**对 `run_command` 做 argv 启发式（`ls` vs `cargo`）作为默认策略——易绕过、难测、与白名单正交。若将来要做，须单独 ADR。
- [ ] 串行 `dispatch_tool` 与并行批读取 **同一套「工具名 → 秒」表**，class 表仅作未点名时的回退。
- [ ] **`terminal_session`**：现状已是 `parallel_tool_wall_timeout_secs`（即默认 `command_timeout_secs`）包整次 exec。P2 **不**改成「无 turn 墙钟」。应：**保留安全网墙钟**；到期或取消时走会话 `close`/`send_signal`，避免只 `timeout` 掉 future 而 PTY 仍占 8 路上限。

**关联文件**：`config/tools.toml`、`cm_config/`、`registry_policy.rs`（可按名查找，不必扩 enum）、工具 JSON Schema（`timeout_secs`）。

**文档**：`docs/配置说明.md` 表；使用者可见新键 / `CM_*` 时改 `README.md`。

---

## 4. P3 — 执行模型、沙盒 reap、观测（可后做）

- [ ] 其余子进程类迁入共享会话；短同步 SDK / 文件 IO 仍 `spawn_blocking`，墙钟按 P2 工具名表。
- [ ] **Docker 沙盒超时 reap**（`docker stop/kill` 或 runner 生命周期）：单独设计，勿与 P0 混 PR。
- [x] **观测**：工具时长直方图、超时率、是否已 kill、残留 Child 计数；日志脱敏（`.cursor/rules/secrets-and-logging.mdc`）。**实现**：`cm_tools/subprocess_session.rs` 进程内原子计数 + 时长直方图（桶上界 1s/5s/30s/120s/600s/溢出），`session_stats_snapshot()` 快照；会话完成打 `debug`（`pid/kind/killed/duration_ms`）、reap 未确认打 `warn`（残留风险），均不含 argv 与密钥。
- [ ] **后台 job + `job_id`**：不绑死当前 LLM turn；须单独契约与兼容窗口。

与路线图「可观测与执行轨迹」、`tool_calling_evolution.md`「长任务进度事件」同向，落地时合并拆解。

---

## 建议落地顺序（PR 切片）

1. **共享会话 + 宿主 `run_command` 杀进程组 + 部分输出进信封**（含 workflow 的 `run_command` 节点）。不改 SSE 顶层键。测试含进程组与管道死锁。
2. **`python_snippet_run` 迁入共享会话**（删除 pid-only hard_kill）；开始迁 `run_and_format*` 中的测试工具。
3. **`run_command`（及已迁入的测试工具）emit `tool_output_chunk`**；Client 气泡对齐。
4. **按工具名墙钟 + 可选 `timeout_secs`**；`terminal_session` 超时/取消 close 会话。
5. 观测；Docker 沙盒 reap ADR；若产品需要再开后台 job ADR。

---

## 完成定义（删 `docs/待办清单.md` 对应条目前）

P0 可先合入，但**整条待办**删除须满足：

- 宿主 `run_command`：超时或取消后，夹具 **进程组** 内无残留（含 `bash -c` 孙进程）。
- 超时/取消 `tool_result` 含截断部分输出与稳定 `error_code`；不写测试缓存、不误标 `workspace_changed`。
- 长 `run_command` 在 Client 工具气泡可见增量（P1）。
- P2：至少 `timeout_secs` 或按工具名覆盖已文档化；**没有**把 `BlockingSync`/`CommandSpawnTimeout` 默认改成 10–30s。
- `docs/工具说明.md` / `docs/配置说明.md`（及若改 SSE：`docs/SSE协议.md` + Client parser）已同步。
- 白名单、路径、审批回归未弱化。

**仍允许残留（须在 PR 写明）**：未迁入会话的 `run_and_format*`、Docker 容器内进程、MCP、非 `run_command` 的 workflow 节点。

---

## 仍开放（实现时写入 PR，不必再改目标）

- 共享模块的精确路径与是否 `tokio::process` 一次到位，还是先 blocking `try_wait` 再迁。
- `timeout_secs` 对 `run_command` 的 Schema 字段名是否与 snippet 完全一致。
- 无输出「已耗时」是否做产品（默认不做新 SSE 键）。
