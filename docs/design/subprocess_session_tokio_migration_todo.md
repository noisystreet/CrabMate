# 子进程会话 tokio 化迁移：实施计划（todo）

> **状态**：设计定稿，**已实施完成**（PR0 调用面收敛、PR1 原语 + chat 同步宿主、PR2 后台 job + workflow、PR3 其余 `run_and_capture` 调用方、PR4 收尾与文档）。本文件回应 [`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md) §共享原语 L102 与 §仍开放 L211–214 的开放项。  
> **受众**：维护 `cm_tools::subprocess_session`、`run_command`（`cm_tools/tools/command.rs`）、`tool_registry/execute_run_command*`、`cm_internal/tool_jobs`、`cm_workflow/execute/node.rs` 的开发者。  
> **语言**：中文。  
> **依据**：决策口径见 [`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md)（§共享原语 / §仍开放）；字段级契约见 [`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)（本迁移**不得**改动其外部行为）。  
> **跟踪**：落地后从 **`docs/待办清单.md`**（`tools/` 章「长耗时工具执行」）删除对应条目，并在 [`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md) §仍开放划掉对应行。

**关联**：

- 长耗时工具执行总纲：[`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md)
- 后台工具任务 ADR / 契约：[`background_tool_jobs.md`](./background_tool_jobs.md) / [`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)
- 实时输出流：[`background_tool_jobs_output_streaming.md`](./background_tool_jobs_output_streaming.md)
- 工具契约与信封：`docs/工具说明.md`；超时与 `[tool_registry]`：`docs/配置说明.md`
- SSE `tool_output_chunk` / `error_code`：`docs/SSE协议.md`
- 安全面：`AGENTS.md` → Repository engineering rules → Security-sensitive surfaces（`run_command` 白名单、路径、审批）

---

## 背景与现状

唯一阻塞原语是 `src/cm_tools/subprocess_session.rs`：

- 走 **`std::process::Command`**：`prepare_piped_process_group`（stdin null、stdout/stderr piped、Unix `process_group(0)`）→ `cmd.spawn()`。
- 等待走 **`spawn_blocking` + 阻塞轮询**：`wait_child_session` 主循环 `flush_live_chunks` → `stop_requested`（`Arc<AtomicBool>` / `extra_stop`）→ 墙钟判定 → `child.try_wait()`，未退出则 `thread::sleep(POLL=50ms)`。
- drain 走 **两个阻塞 `thread::spawn`**（`spawn_drain`），`take_drain` 用 `thread::join` + `DRAIN_JOIN` 超时兜底；每次 `flush_live_chunks` 把新字节喂给 `chunk_sink`，返回 `false` 时该增量入队重试（SSE `try_send` 背压语义）。
- 超时/取消：`terminate_child_group` → `unix_term_then_kill`（`nix` `killpg SIGTERM` → `sleep(TERM_GRACE=400ms)` → `killpg SIGKILL` → `kill(child)`），再 `reap_after_kill`（`REAP_WAIT=2s` 有界 reap）。

**宿主调用面**（均最终落到 `wait_child_session` / `run_and_capture`）：

| 宿主 | 文件 | 形态 |
|---|---|---|
| chat `run_command`（同步） | `cm_internal/tool_registry/execute/execute_run_command_sync.inc.rs` | `tokio::task::spawn_blocking` 内调 `tools::run_checked_wait`，`with_request_github_token_blocking` 包一层 |
| chat `run_command`（`async:true` 后台） | `execute_run_command_async.inc.rs` → `cm_internal/tool_jobs::worker::launch_job` | `tokio::spawn` 内再 `spawn_blocking` + `catch_unwind` 调 `run_job_blocking` |
| workflow 节点 | `cm_workflow/execute/node.rs` | `async` 块内 `spawn_blocking` 调 `run_tool_result` |
| `python_snippet_run` | `cm_tools/tools/python_tools.rs` | 直接 `prepare_piped_process_group` + `wait_child_session`；`run_and_capture` |
| `cargo_test` / `pytest_run` 等 | `cm_tools/tools/cargo_subcommand.rs` 等 | `run_and_capture` |
| 错误 playbook | `cm_tools/tools/error_playbook.rs:480` | `run_checked_wait` |
| replay / cli_mcp | `runtime/tool_replay.rs` / `runtime/cli_mcp.rs` | 同步 CLI 上下文内 `run_tool` |

**问题**：

1. 长命令**占死 `spawn_blocking` 线程**（每个会话一个线程，直到墙钟到期）；阻塞线程池被打满后，其它需 `spawn_blocking` 的工具全部排队。
2. 50ms 轮询是**忙等**，取消/超时确认存在最多 `POLL` 级延迟。
3. 模块头注释（`subprocess_session.rs` L3–4）已显式标记「当前实现走阻塞轮询……迁 `tokio::process` 见 todo」，本次即收口。

---

## 目标与非目标

**目标**：

- 将 `wait_child_session` / `run_and_capture` 及 `command.rs` 的 `run_*` API 面改为 **`async fn`**，底层改用 **`tokio::process`**，消除阻塞轮询与 `spawn_blocking` 占用。
- 保留**全部外部语义**：进程组 SIGTERM→SIGKILL、输出截断、并发 drain、`chunk_sink` 背压（`false` 入队重试）、会话统计、`SessionStopKind`、取消（含 SSE closed 的 `extra_stop`）。
- chat 两路宿主、job worker、workflow 节点改为**直接 `await`**，去掉 `spawn_blocking` 包装。

**非目标**：

- 不改白名单 / `..` / 绝对路径 / 审批门闩（安全红线，见下）。
- 不改 SSE 协议、不 bump `SSE_PROTOCOL_VERSION`、不改 `tool_result` 软字段。
- 不改 **Docker 沙盒容器内 runner**（`cm_internal/tool_sandbox/runner.rs`）与 **MCP** 子进程。
- 不引入并发上限 / 作业队列新模型（沿用 `tool_jobs` 现有并发/排队/上限）。
- 不做 Windows 进程组等价物（保持 Unix 优先，沿用既有 `#[cfg(unix)]` 分支策略）。

---

## 核心决策

- **D1（一次到位）**：直接迁 `tokio::process`，**不**走「先 blocking `try_wait` 再迁」的中间态（回应 L213 开放项）。理由：中间态仍占线程池，收益有限且要改两次调用面。
- **D2（收敛在原语）**：所有 wait 仍只有一个实现（`subprocess_session`），调用方只做 `async` 化，禁止各自抄一套 `tokio::process` wait。
- **D3（契约最小变）**：`SubprocessWaitCtl`、`SessionStopKind`、`SessionWaitResult`、`SessionStream`、`SessionChunkSink` 的字段与语义**保持不变**，仅函数签名加 `async`。
- **D4（取消升级）**：取消从「`AtomicBool` 轮询」升级为「`Notify`（可选 future）」；`Arc<AtomicBool>` 入口**保留兼容**（一次性过渡），但主循环不再靠 `sleep(POLL)` 感知。
- **D5（kill 沿用 nix）**：进程组信号仍走 `nix` `killpg`；仅把 `thread::sleep` 换成 `tokio::time::sleep`。`child.kill()`（tokio 版）只在直接子进程兜底，**不**替代进程组 kill。

---

## async 原语设计

### 构建期（保持同步）

`prepare_piped_process_group(&mut std::process::Command)` **不**改：仍在 std `Command` 上设 stdin null / stdout+stderr piped / `#[cfg(unix)] process_group(0)`。构建命令时插入的 `GH_TOKEN` 等 env（`github_token::apply_gh_token_env*`）也在这一层。

### spawn

```rust
// std_cmd: std::process::Command（已 prepare_piped_process_group + env）
let mut cmd = tokio::process::Command::from_std(std_cmd)?;
cmd.kill_on_drop(true);
let mut child = cmd.spawn()?; // async
```

- `from_std` 在 Unix 上保留 `process_group(0)`（tokio 内部仍以 std 的 spawn 配置落盘）——**PR 需加断言/回归用例核实**（见风险）。
- `kill_on_drop(true)` 是**兜底**：正常情况下走显式 kill+reap；`kill_on_drop` 只在会话对象被 drop 且未 kill 时触发 SIGKILL（防 panic 泄漏）。

### drain（并发、不占阻塞线程）

用 tokio 版 `ChildStdout` / `ChildStderr`（实现 `AsyncRead`），**每路一个 `tokio::spawn` task**：

```rust
// kept: 累计（受 cap 截断）；live: 待 flush 增量；notify: 唤醒主循环
loop {
    match reader.read(&mut buf).await? {
        0 => break,
        n => { record(&mut kept, &buf[..n], cap); push_live(&buf[..n]); notify.notify_one(); }
    }
}
```

- 与现状一致：drain task 只**搬运/缓冲**，**不**直接调 `chunk_sink`（`chunk_sink` 是同步 `Fn`，且需「返回 `false` 则入队重试」的背压语义）。
- `live` 仍用 `Arc<Mutex<VecDeque<DrainEvent>>>`；`notify` 用 `tokio::sync::Notify` 让主循环在新字节到达时立刻 flush（替代 50ms 轮询）。
- 结束条件：EOF 或进程被杀后管道关闭。join 用 `tokio::time::timeout(DRAIN_JOIN, handle)` 兜底，超时带走已捕获字节（保持现状语义）。

### wait（`select!`）

主循环改为事件驱动：

```rust
loop {
    flush_live_chunks(live.as_ref(), ctl.chunk_sink.as_ref()); // 同步，非阻塞
    tokio::select! {
        biased;
        st = child.wait() => { kind = Exited; break st; }
        _ = sleep_until(deadline), if deadline.is_some() => {
            kind = Timeout; kill_group(); break Ok(exit-stub);
        }
        _ = cancel.notified() => { kind = Cancelled; kill_group(); break ...; }
        _ = live_notify.notified() => { /* 回环顶部 flush */ }
        _ = wait_extra_stop(ctl) => { kind = Cancelled; kill_group(); break ...; }
    }
}
```

- `sleep_until(deadline)`：`ctl.wall` 为 `None` 时该分支禁用（`if` guard），与现状「无墙钟靠外圈 timeout」一致。
- `cancel`：由 `Arc<AtomicBool>` + `Notify` 组成——`stop_requested(ctl)` 仍读 `AtomicBool`（过渡期其它代码可能直接置位），同时在**置位处** `notify`；`select!` 主分支等 `notified()`，进入后即刻用 `stop_requested` 复判，避免误判。
- `wait_extra_stop`：`extra_stop: Arc<dyn Fn()->bool>` 语义不变；**可选增强**——新增 `extra_stop_fut: Option<Pin<Box<dyn Future<Output=()>+Send>>>`（chat 传 SSE `Sender::closed()`），消除对 `is_closed` 的轮询。**PR1 先保留 `Fn` 轮询 + `sleep` 退避**，`Future` 作为后续切片。

### kill + reap

```rust
fn kill_group(pid) { unix_term_then_kill(pid); }        // nix killpg TERM→tokio::sleep(TERM_GRACE)→KILL
// reap：有界
match tokio::time::timeout(REAP_WAIT, child.wait()).await { ... }
```

- `unix_term_then_kill` 内部两处 `thread::sleep` → `tokio::time::sleep`（因此该 fn 也变 `async`，或拆出 sync 版仅在「无可 await 上下文」使用）。
- `reap_after_kill` → `child.wait().await` 包 `timeout(REAP_WAIT)`；超时后再 `child.kill()` 并短等，语义与现状 `reap_after_kill` 一致。
- `terminate_child_group` / `reap_after_kill` / `unix_term_then_kill` 全部 `async` 化。

### 返回契约

`SessionWaitResult` / `SessionStopKind` / `SessionStats` **字段不变**；`wait_child_session` / `run_and_capture` 仅签名变 `async fn`。计数与直方图（`record_session_stats`）读写点不变。

### chunk_sink 并发边界

`SessionChunkSink = Arc<dyn Fn(SessionStream,&[u8])->bool + Send+Sync>` **保持不变**。约束：

- 只在**主循环任务**内调用（单线程序列化），drain task **只**写 `live` 队列并 `notify`——因此 `chunk_sink` 无需 `Send` 之外的新约束，也无需改动 SSE 侧 `emit_run_command_tool_output_chunk` 的 `AtomicU64`/`Mutex`。
- `finish_chunk_sink`（发空增量收尾）在 reap 之后、返回之前调用一次，位置不变。

---

## 调用方改造清单

| # | 文件 | 现状 | 改造 |
|---|---|---|---|
| 1 | `cm_tools/subprocess_session.rs` | `wait_child_session` / `run_and_capture` / `spawn_drain` / `take_drain` / `terminate_child_group` / `reap_after_kill` / `unix_term_then_kill` 同步 | 全部 `async`；`thread::{sleep,spawn,join}` → `tokio::{time::sleep/spawn/time::timeout}`；模块头注释更新（删「阻塞轮询」） |
| 2 | `cm_tools/tools/command.rs` | `run_try` / `run_try_wait` / `run_checked` / `run_checked_wait` / `run_impl` / `run_spawned_command` 同步（`run_with_wait` 已随 PR0 删除） | 全部 `async fn`，内部 `.await`；`RunCommandError::TimedOut` 映射不变 |
| 3 | `cm_internal/tool_registry/execute/execute_run_command_sync.inc.rs` | `spawn_blocking` + `with_request_github_token_blocking` + `tools::run_checked_wait` | 去 `spawn_blocking`，直接 `await tools::run_checked_wait(...)`；token 改 `with_request_github_token`（见「坑」） |
| 4 | `cm_internal/tool_jobs/worker.rs` | **PR2 已实施**：原 `run_job_blocking`（同步）+ `launch_job` 的 `spawn_blocking` + `catch_unwind` | 改名 `run_job_async`，`prepare_piped_process_group` + `wait_child_session_async` 全 `await`；`launch_job` 去 `spawn_blocking`，panic 由内层 `tokio::spawn` 的 `JoinError` 兜底（保留 terminate 进程组后再标 `failed(internal)` 的逻辑）；`chunk_sink_from_output` 不变；命令转 `tokio::process::Command` 后 `kill_on_drop(true)` 兜底 abort/drop |
| 5 | `cm_workflow/execute/node.rs` | **PR0 / S5 已实施**：原 `spawn_blocking` 调 `run_tool_result`；`run_command_reaps_self` 特判 | run_command 节点：走 `async` 路径直接 `await`；非 run_command 的同步工具仍可保留 `spawn_blocking`；「非 run_command 无法取消」警告相应修订 |
| 6 | `cm_tools/tools/python_tools.rs` | **PR3 已实施**：路线 A 保留同步入口；L357–374 `prepare_piped_process_group`+`wait_child_session`；L475 `run_and_capture` | **PR0 决策**（S8）：路线 A 保留同步入口；路线 B `await`（需 `run_tool` 一并 async 化）。见「调用面收敛」 |
| 7 | `cm_tools/tools/cargo_subcommand.rs` | **PR3 已实施**：路线 A 保留同步入口；L184 `run_and_capture` | 同 #6 |
| 8 | `cm_tools/tools/error_playbook.rs` | **PR3 已实施**：路线 A 保留同步入口；L482 `run_checked_wait` | 同 #6；路线 A 下保留外层 `spawn_blocking`（本 PR 允许残留并写明） |
| 9 | `cm_tools/tools/mod.rs` | `run_tool` / `run_tool_dispatch`（**同步**）中 `"run_command"` 特判走 `run_try_wait` | **PR0 / S1**：消费点 re-route 后删除特判 |
| 10 | `cm_tools/tools/runners.rs` | `runner_run_command`（`ToolRunner::Legacy`，因 `#9` 特判实际已近似死代码） | **PR0 / S2**：确认为死代码 → 置提示性 stub（spec runner 注册占位，范本同 `runner_terminal_session`），不再执行命令；**后续清理**删除 stub，改由 `ToolRunner::AsyncOnly` 表达 |
| 11 | `runtime/cli/commands.rs`（sync 入口）/ `runtime/tool_replay.rs`（`run_tool` 调用）/ `runtime/cli_mcp.rs` | 同步上下文内 `run_tool` | **PR0 / S3、S4**：S3 改 `async fn`；S4 改 `ToolCallbacks` 契约。见「调用面收敛」 |
| 12 | `cm_internal/tool_sandbox/runner.rs`（+ `src/main.rs`） | 同步 `dispatch_tool_runner_kind` 内 `run_tool(kind="run_command")` | **PR0 决策 / S6**：路线 (a) runtime 提前 + `block_on`；路线 (b) 保留 sync 入口。见「调用面收敛」 |
| 13 | 测试 | `subprocess_session_tests.rs`、`tools/command_run_tests.rs`、`tools/mod/tests.rs` | 相关用例改 `#[tokio::test]`（或 `#[tokio::test(flavor="multi_thread")]`，覆盖 drain 并发） |

### 调用面收敛（前置 PR0，迁移主要阻碍）

**事实核对（先去掉误判）**：chat 的 `run_command` 走 `HandlerId::RunCommand` → `execute_run_command_impl`（[execute_dispatch_body.inc.rs](../../src/cm_internal/tool_registry/execute/execute_dispatch_body.inc.rs#L388-L393)），**不**经过 `run_tool*`。`run_tool_dispatch` 里的 `"run_command"` 特判只服务**非 chat** 的同步消费点。因此收敛面**有限且可枚举**：

| # | 同步消费点 | 定义位置 | 可命中 `run_command` | 收敛动作 | 成本 |
|---|---|---|---|---|---|
| S1 | `run_tool_dispatch` 的 `"run_command"` 特判（`run_try_wait`） | [mod.rs](../../src/cm_tools/tools/mod.rs#L441-L462) | 是 | 消费点全部 re-route 后**删除特判** | 低 |
| S2 | Legacy `runner_run_command`（`ToolRunner::Legacy`） | [runners.rs](../../src/cm_tools/tools/runners.rs) | 是（`run_tool` fallback） | 置提示性 stub（spec 注册占位，不再执行命令；stub 后续已删除，改由 `ToolRunner::AsyncOnly` 表达） | 低 |
| S3 | `tool-replay` CLI | [commands.rs](../../src/runtime/cli/commands.rs#L14-L18)，调用点 [cli_run.rs](../../src/cli_run.rs#L145) | 是（fixture 步骤名） | `run_tool_replay_command` 改 `async fn`，调用点 `await`（宿主 [main.rs](../../src/main.rs#L32) 已 `block_on`） | 低 |
| S4 | MCP `tools/call` 回调 | [cli_mcp.rs](../../src/runtime/cli_mcp.rs#L38)；契约 [server.rs](../../src/cm_mcp/mcp_impl/server.rs#L31-L32)，调用 [server.rs](../../src/cm_mcp/mcp_impl/server.rs#L151) | 是（客户端可传 `run_command`） | `ToolCallbacks::run_tool` 签名改返回 `Future`；`call_tool` 内 `await` | 中（改 MCP 契约） |
| S5 | workflow 节点 | [node.rs](../../src/cm_workflow/execute/node.rs#L58-L95) | 是 | `run_command` 节点走 async 直连；其余工具仍可保留 `spawn_blocking` | 低 |
| S6 | 沙盒容器 runner | [runner.rs](../../src/cm_internal/tool_sandbox/runner.rs#L181-L186)，入口 [main.rs](../../src/main.rs#L7-L13) | 是 | 见下「容器 runner」 | 中 |
| S7 | chat `SyncDefault` 兜底（inline / `spawn_blocking` 两处 `run_tool`） | [execute_dispatch_body.inc.rs](../../src/cm_internal/tool_registry/execute/execute_dispatch_body.inc.rs#L154)、[同文件](../../src/cm_internal/tool_registry/execute/execute_dispatch_body.inc.rs#L189) | **否**（已被 `HandlerId` 分流） | 本次无需为 `run_command` 改动 | — |

> `sync_default_runs_inline` 的内联集合原为 `get_current_time` / `convert_units`，二者已于 2026-10-24 移除、现默认为空集（见 [registry_policy.rs](../../src/cm_tools/registry_policy.rs#L292-L300)），不涉及子进程工具。

**这不是 `run_command` 独有问题**——下列同步子进程工具同样挂在 `run_tool` 面上，共同决定「`run_tool` 是否要整体 async 化」的边界：

| 工具 | 位置 | 现状 |
|---|---|---|
| `python_snippet_run` | [python_tools.rs](../../src/cm_tools/tools/python_tools.rs#L357-L372) | 同步 `prepare_piped_process_group` + `wait_child_session` |
| `pytest_run` | [python_tools.rs](../../src/cm_tools/tools/python_tools.rs#L471) | `run_and_capture` |
| `cargo_test` 等 | [cargo_subcommand.rs](../../src/cm_tools/tools/cargo_subcommand.rs#L182) | `run_and_capture` |
| `error_output_playbook` | [error_playbook.rs](../../src/cm_tools/tools/error_playbook.rs#L480) | `run_checked_wait` |

**路线决策（PR0 二选一，须写进 PR 描述）**：

- **路线 A（推荐，最小改动）**：只把 **`run_command`** 从同步 `run_tool*` 面摘除（S1/S2 删除 + S3/S4/S5/S6 re-route 到 async 面）。上表同步子进程工具与容器 runner **本次不动**，继续使用 `subprocess_session` 的**同步等待入口**。
  - 代价：`subprocess_session` 需同时保留 sync + async 两个 wait 入口。这是对 **D2「单一实现」的有限让步**——sync 入口须设计为**薄封装**（复用同一 `DrainEvent`/`live` 队列与 kill/drain 逻辑，仅 `try_wait`+`sleep` 与 `child.wait().await`+`select!` 不同），PR 描述注明过渡性质。
- **路线 B（彻底）**：`run_tool*` 整体 async 化，连带所有 runner / 上表工具 / 容器 runner 一并改造。
  - 与 D1「一次到位」一致，但 PR1 体积大、`run_tool` 契约变更波及面广（含 [web/workspace/handlers.rs](../../src/web/workspace/handlers.rs#L419)、[execute_http_tools.inc.rs](../../src/cm_internal/tool_registry/execute/execute_http_tools.inc.rs#L139) 等同步调用点）。

### 路线 A 落地形式：`run_tool_async` 专用入口（两阶段 strangler）

**已实施（PR0 完成）**。路线 A 的具体化：不改 `run_tool*` 整体契约，而是在 `cm_tools/tools` 新建**通用 async 入口**，把非 chat 消费点整体迁移过去——`run_command` 与历史同步面彻底解耦，消费点不再散落 `if name == "run_command"` 分支。

- **接口**（[mod.rs](../../src/cm_tools/tools/mod.rs#L539-L564)）：新增
  - `pub async fn run_tool_async(name, args_json, ctx) -> String`
  - `pub async fn run_tool_result_async(name, args_json, ctx) -> ToolResult`
  - 两入口内部：`name == "run_command"` 走新实现 `run_command_dispatch_async`；**其余工具直接委派**既有同步 `run_tool` / `run_tool_result`，行为逐字节不变。
- **两阶段 strangler**：
  - **阶段 1（本 PR0）**：`run_command_dispatch_async` 实现体用 `tokio::task::spawn_blocking` 复用既有同步 `command::run_try_wait`（含 `command_timeout_secs` 墙钟超时与测试缓存）；spawn 前捕获 github token，闭包内 `with_request_github_token_blocking` 重设（task-local 不跨 blocking，对齐 chat 宿主 `spawn_run_command_host_blocking`）；成功/失败收尾复用 `finish_dispatch_legacy`。S3/S4/S5/S6 消费点全部改调 async 入口；随后删除 S1 特判、S2 `runner_run_command`（spec runner 置提示性 stub，范本同 `runner_terminal_session`；stub 后续亦删除，改由 `ToolRunner::AsyncOnly` 表达）。风险小、可回滚。
  - **阶段 2（后续 PR1）**：仅把 `run_command_dispatch_async` 实现体换为 `tokio::process` 事件驱动版本，**接口契约不变**，S3–S6 消费点零改动。
- **消费点迁移落点**：S3 [tool_replay.rs](../../src/runtime/tool_replay.rs) + [commands.rs](../../src/runtime/cli/commands.rs) + [cli_run.rs](../../src/cli_run.rs)；S4 [cli_mcp.rs](../../src/runtime/cli_mcp.rs)（`ToolCallbacks::run_tool` 改 `BoxFuture<'static, _>`）+ [server.rs](../../src/cm_mcp/mcp_impl/server.rs)；S5 [node.rs](../../src/cm_workflow/execute/node.rs)；S6 [runner.rs](../../src/cm_internal/tool_sandbox/runner.rs) + [main.rs](../../src/main.rs)。

**容器 runner（S6）与 D1 的冲突（必须在 PR0 定路线）**：

[main.rs](../../src/main.rs#L5-L13) 在**构建 tokio runtime 之前**处理 `tool-runner-internal`，容器内走同步 `tool_runner_internal_main()` → `run_tool("run_command")` → `command.rs` 同步 API。因此「`command.rs` 全 async」与清单 #12 的「容器 runner 保留原样」**不能同时成立**。二选一：

- **(a) 推荐**：把 [main.rs](../../src/main.rs#L15-L24) 的 runtime 构建提到 `tool-runner-internal` 分支之前，`tool_runner_internal_main` 改 `async` 并以 `block_on` 驱动（影响面仅该二进制内的一次性调用）。
- **(b)** 容器 runner 保留同步 `command.rs`（仅此一处），即依赖**路线 A** 的 sync 入口。

> **PR0 出口条件（已达成）**：`run_tool_dispatch("run_command")` 特判与 `runner_run_command` 已删除（S2 stub 后续亦删除，改由 `ToolRunner::AsyncOnly` 表达）；S3/S4/S5/S6 均已 re-route 或明确保留 sync 入口；PR1 的 async 化不再被同步编译依赖阻塞。PR0 仅做「re-route + 删死代码」，**不改** `subprocess_session` / `command.rs` 内部实现，因而**独立可构建、可回滚**。

---

## 三个坑（必须显式处理）

### 坑 1：GitHub token 的 `task_local`

`cm_tools/github_token.rs` 用 `tokio::task_local!` 存 `REQUEST_GITHUB_TOKEN`。

- 现状：`spawn_blocking` 内用 `with_request_github_token_blocking`（`sync_scope`），因为 task-local **不会**跨到阻塞线程。
- 迁移后：改回 `with_request_github_token`（`REQUEST_GITHUB_TOKEN.scope(...).await`）。
- **注意**：`tokio::spawn` 出的**新任务不继承** task-local。当前命令构建（`apply_gh_token_env_if_gh_command` / `resolve_token_for_child_env`）发生在**发起任务的当前作用域内**，故安全；**drain task 与 wait 不得**再依赖 task-local。PR 需在 `execute_run_command_sync.inc.rs` 与 `worker.rs` 两处显式核对 token 取值时机。

### 坑 2：`chunk_sink` 并发与背压

- `chunk_sink` 返回 `false` 表示该增量**未**成功投递（SSE `try_send` 满），须入队重试——这个语义**必须**在新主循环里保留，否则会丢工具增量。
- 约定：**只有主循环任务**调用 `chunk_sink`；drain task 只写 `live` 队列 + `notify`。这样 `chunk_sink` 的单线程序列化性质不变，SSE 侧 `seq`/`utf8_pending` 的原子与锁语义无需改动。
- `uncapped_live`（`true` 时不按 `cap` 截断 live）语义保留，用于后台 job 实时流。

### 坑 3：语义红线（迁移**不得**触碰）

- 改 wait **不得**绕过白名单、`..`/绝对路径校验、审批门闩。
- 超时 / 取消 **不得**走 `is_compile_command_success` 成功路径，**不得**写 `test_result_cache`，**不得**把 `workspace_changed` 打成 `true`。
- 超时 / 取消的 `error_code` 稳定（`timeout` / 取消），部分输出随正文返回。
- 日志脱敏：不打 argv 密钥。

---

## PR 切片

1. **PR0 — 调用面收敛（前置，独立可回滚）**：按上文「调用面收敛」节 re-route 同步消费点——S3 `run_tool_replay_command` 改 `async fn`（调用点 `await`）；S4 `ToolCallbacks::run_tool` 契约改返回 `Future`；S5 workflow `run_command` 节点改 async 直连；S6 容器 runner 定路线（(a) runtime 提前 + `block_on` / (b) 保留 sync 入口）；随后**删除** S1 `run_tool_dispatch` 的 `"run_command"` 特判与 S2 Legacy `runner_run_command`。**本切片确定路线 A/B 并写进 PR 描述**。仅做 re-route + 删死代码，**不改** `subprocess_session` / `command.rs` 内部实现，独立可构建、可回滚。
   - **已实施**：路线取 **A**，落地形式为「`run_tool_async` / `run_tool_result_async` 专用入口 + 两阶段 strangler」（见上「路线 A 落地形式」）；S6 取路线 **(a)**；S1/S2 已删除，受影响单测（`tools/mod/tests.rs`）已迁移至 `#[tokio::test]` + async 入口。
2. **PR1 — 原语 + chat 同步宿主**：`subprocess_session` / `command.rs` async 化；`execute_run_command_sync.inc.rs` 去 `spawn_blocking`，token 改 async 版；模块头注释更新；`subprocess_session_tests.rs` 改 `#[tokio::test]`。路线已由 **PR0** 决定（见上）。
   - **已实施**：新增 `wait_child_session_async` 与 `command.rs` async 入口（`run_checked_wait_async` / `run_try_wait_async`），sync / async 共用同一 plan 层；chat 宿主与异步 funnel 去 `spawn_blocking`（token 走 task-local，无需 `*_blocking`）；`subprocess_session_tests.rs` 增 async 用例。**未**删同步 `wait_child_session`（路线 A 保留同步入口，供非 `run_command` 工具与 PR3 调用方复用）。
3. **PR2 — 后台 job + workflow**：`worker.rs` `run_job_async`；`launch_job` 去 `spawn_blocking`；`node.rs` run_command 节点 async。
   - **已实施**：`worker.rs` `run_job_blocking` → `run_job_async`（`tokio::process::Command` + `kill_on_drop(true)` + `wait_child_session_async`）；`launch_job` 去 `spawn_blocking` / `catch_unwind`，改内层 `tokio::spawn(run_job_async(...)).await`，`JoinError` → `failed(internal)`（不卡 `running`）；`mod.rs` 导出同步改名。`node.rs` run_command 节点在 **PR0 / S5** 已 async 直连，本 PR 无改动；测试改 `#[tokio::test]`（`thread::sleep` → `tokio::time::sleep`）。
4. **PR3 — 其余 `run_and_capture` 调用方**：`python_tools.rs` / `cargo_subcommand.rs` / `error_playbook.rs`（路线 A 下注明残留 sync 入口）。
   - **已实施**：三个调用方在路线 A 下**保持同步** `run_tool` 面，沿用 `subprocess_session` 同步等待入口（`wait_child_session` / `run_and_capture` / `command::run_checked_wait`）；在三处调用点补注释说明残留 sync 入口的过渡性质。**未**改异步（需 `run_tool*` 整体 async 化，见「路线 B」）。
5. **PR4 — 收尾与文档**：`docs/开发文档.md`（若涉及执行类别/线程模型描述）、`docs/design/README.md` 计数、`long_running_tool_execution_todo.md` §仍开放划项。
   - **已实施**：`long_running_tool_execution_todo.md` §仍开放「是否 `tokio::process` 一次到位」一行已划项并标注 PR0–PR4 落地；本文件状态改「已实施完成」、§风险与开放项标注路线已决。`docs/开发文档.md` 经核对无执行类别 / 线程模型 / `spawn_blocking` 描述，**无需改动**；`docs/design/README.md` §2 计数（8）已正确，**无需改动**。

> 每个 PR 独立可构建、可测试、可回滚；**PR0** 合入后 `run_command` 不再有新的同步调用方，PR1 起仅做原语与宿主 async 化。

---

## 测试

- **改**：`subprocess_session_tests.rs`（`#[tokio::test(flavor="multi_thread")]`，覆盖 drain 并发）、`command_run_tests.rs`、`tools/mod/tests.rs`。
- **新增**：
  - async 取消**即时性**：置位后不再有 `POLL` 级延迟（用 `Notify` 唤醒）。
  - 墙钟超时对**进程组** SIGTERM→SIGKILL（含 `bash -c` 孙进程），reap 后进程组无残留。
  - `chunk_sink` 返回 `false` → 增量入队重试，不丢字节。
  - `kill_on_drop` 兜底：会话对象 drop 后子进程被杀。
- **回归（红线）**：白名单 / `..` / 绝对路径 / 审批；超时或取消**不**误判成功、**不**写测试缓存、**不**置 `workspace_changed`；部分输出随正文返回且 `error_code` 稳定。

---

## 文档同步

- **已完成（PR1）**：`subprocess_session.rs` 模块头注释删「当前实现走 `std::process` + 阻塞轮询」，改述 tokio 模型（sync + async 双入口）。
- **已完成（PR4）**：[`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md) §仍开放 L211–214：已划掉「是否 `tokio::process` 一次到位」一行。
- **已核对（PR4）**：**`docs/开发文档.md`** 未描述执行类别 / `spawn_blocking` 策略 / 线程模型，无需同步（若后续新增相关描述，按 `AGENTS.md` → Repository engineering rules → Architecture & documentation sync 同步）。
- **已登记**：本文件登记于 `docs/design/README.md` §2。

---

## 风险与开放项

- **`from_std` 是否稳定保留 `process_group(0)`**：需回归用例核实；若不可靠，改为「spawn 前用 `pre_exec`（`unsafe`）显式 `setpgid(0,0)`」的备选路径。
- **`kill_on_drop` 与显式 kill 的关系**：避免重复 kill 造成的 `ESRCH` 噪声；约定显式 kill 后再置 `kill_on_drop` 无害化（或 kill 成功后由 reap 接管）。
- **同步 `run_tool*` 面的最终形态**：**已定稿（PR0）**：取 **路线 A**（推荐）保留「非 `run_command` 同步入口 + `run_command` 走 async 直连」，代价是 `subprocess_session` 短期并存 sync/async 两个 wait 入口（薄封装，D2 的有限让步），已由 PR0–PR3 落地；**路线 B** 将 `run_tool*` 整体 async 化，波及 [web/workspace/handlers.rs](../../src/web/workspace/handlers.rs#L419)、[execute_http_tools.inc.rs](../../src/cm_internal/tool_registry/execute/execute_http_tools.inc.rs#L139) 等同步调用点，本次**未**采用。
- **容器 runner（S6）与 D1 的时序冲突**：[main.rs](../../src/main.rs#L5-L13) 在 runtime 构建前处理 `tool-runner-internal`，故「`command.rs` 全 async」与「容器 runner 保留原样」不能并存；**已定稿（PR0）**：取路线 **(a)** runtime 提前 + `block_on`。
- **`extra_stop` 是否升级为 `Future`**：PR1 先保留 `Fn` 轮询（`sleep` 退避），`Future`（SSE `Sender::closed()`）作为后续增强切片；需评估对 workflow 只传超时场景的接口影响。
- **Windows**：`from_std` 无 `process_group` 等价物；本次保持 Unix 优先，Windows 分支维持现状（`child.kill()` 兜底）。
