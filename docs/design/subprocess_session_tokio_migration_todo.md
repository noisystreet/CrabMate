# 子进程会话 tokio 化迁移：实施计划（todo）

> **状态**：设计定稿，**待实施**（尚未改动任何代码）。本文件回应 [`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md) §共享原语 L102 与 §仍开放 L211–214 的开放项。  
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
- 安全面：`.cursor/rules/security-sensitive-surface.mdc`（`run_command` 白名单、路径、审批）

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
| 2 | `cm_tools/tools/command.rs` | `run_try` / `run_try_wait` / `run_checked` / `run_with_wait` / `run_checked_wait` / `run_impl` / `run_spawned_command` 同步 | 全部 `async fn`，内部 `.await`；`RunCommandError::TimedOut` 映射不变 |
| 3 | `cm_internal/tool_registry/execute/execute_run_command_sync.inc.rs` | `spawn_blocking` + `with_request_github_token_blocking` + `tools::run_checked_wait` | 去 `spawn_blocking`，直接 `await tools::run_checked_wait(...)`；token 改 `with_request_github_token`（见「坑」） |
| 4 | `cm_internal/tool_jobs/worker.rs` | `run_job_blocking`（同步）+ `launch_job` 的 `spawn_blocking` + `catch_unwind` | 改名 `run_job_async`，`prepare_piped_process_group` + `wait_child_session` 全 `await`；`launch_job` 去 `spawn_blocking`，panic 由 `tokio::spawn` 的 `JoinError` 兜底（保留 terminate 进程组后再标 `failed(internal)` 的逻辑）；`chunk_sink_from_output` 不变 |
| 5 | `cm_workflow/execute/node.rs` | `spawn_blocking` 调 `run_tool_result`；`run_command_reaps_self` 特判 | run_command 节点：走 `async` 路径直接 `await`；非 run_command 的同步工具仍可保留 `spawn_blocking`；「非 run_command 无法取消」警告相应修订 |
| 6 | `cm_tools/tools/python_tools.rs` | L455–473 `prepare_piped_process_group`+`wait_child_session`；L581 `run_and_capture` | `await`（调用点若为同步 API 需一并 async 化，见「同步面」） |
| 7 | `cm_tools/tools/cargo_subcommand.rs` | L182 `run_and_capture` | `await` |
| 8 | `cm_tools/tools/error_playbook.rs` | L480 `run_checked_wait` | 视调用上下文 `await`，或保留外层 `spawn_blocking`（本 PR 允许残留并写明） |
| 9 | `cm_tools/tools/mod.rs` | `run_tool` / `run_tool_dispatch`（**同步**）中 `"run_command"` 特判走 `run_try_wait` | 关键阻碍，见「同步 `run_tool*` 面」 |
| 10 | `cm_tools/tools/runners.rs` | `runner_run_command`（`ToolRunner::Legacy`，因 `#9` 特判实际已近似死代码） | 迁移后确认无引用即**删除** |
| 11 | `runtime/tool_replay.rs` / `runtime/cli_mcp.rs` | 同步 CLI 上下文内 `run_tool` | 见「同步 `run_tool*` 面」 |
| 12 | `cm_internal/tool_sandbox/runner.rs` | 同步 `dispatch_tool_runner_kind` 内 `run_tool(kind="run_command")` | **超范围**：容器内 runner 不在本次迁移，保留原样并在 PR 写明 |
| 13 | 测试 | `subprocess_session_tests.rs`、`tools/command_run_tests.rs`、`tools/mod/tests.rs` | 相关用例改 `#[tokio::test]`（或 `#[tokio::test(flavor="multi_thread")]`，覆盖 drain 并发） |

### 同步 `run_tool*` 面（迁移主要阻碍）

`run_tool` / `run_tool_dispatch` / `run_tool_result` 目前是**同步 API**，被 replay CLI、`cli_mcp` 闭包、沙盒 runner、单测等多处消费，无法在其内部直接 `await`。收敛策略（**首选**）：

1. **`run_command` 从同步 `run_tool_dispatch` 移除特判**：chat 两路宿主与 workflow 节点已改为直连 async `command::run_*`，不再经 `run_tool_dispatch("run_command")`。
2. **同步残留中真正需要 run_command 的入口**（如 replay CLI）改为 `#[tokio::main] async fn main`（或局部 `tokio::runtime::Handle::block_on`），使其能 `await` 新 API；**不做**「同步 API 内部 `block_on`」这种折衷（会在 async 宿主中 panic）。
3. `run_tool_dispatch_fallback` 与 Legacy `runner_run_command`：确认无调用后清理。

> 若上述第 2 步成本过高，退路：保留 `run_tool_dispatch` 仅服务**非 run_command** 工具，run_command 的同步入口彻底删除。**PR1 必须先把这条路线在 PR 描述里二选一确认**。

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

1. **PR1 — 原语 + chat 同步宿主**：`subprocess_session` / `command.rs` async 化；`execute_run_command_sync.inc.rs` 去 `spawn_blocking`，token 改 async 版；模块头注释更新；`subprocess_session_tests.rs` 改 `#[tokio::test]`。**本切片决定「同步 `run_tool*` 面」路线**（见上）。
2. **PR2 — 后台 job + workflow**：`worker.rs` `run_job_async`；`launch_job` 去 `spawn_blocking`；`node.rs` run_command 节点 async。
3. **PR3 — 其余 `run_and_capture` 调用方**：`python_tools.rs` / `cargo_subcommand.rs` / `error_playbook.rs`（或注明残留）；清理 Legacy `runner_run_command`。
4. **PR4 — 收尾与文档**：`docs/开发文档.md`（若涉及执行类别/线程模型描述）、`docs/design/README.md` 计数、`long_running_tool_execution_todo.md` §仍开放划项。

> 每个 PR 独立可构建、可测试、可回滚；PR1 合入后即不再新增阻塞 wait 的新调用方。

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

- `subprocess_session.rs` 模块头注释删「当前实现走 `std::process` + 阻塞轮询」，改述 tokio 模型。
- [`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md) §仍开放 L211–214：划掉「是否 `tokio::process` 一次到位」一行。
- **`docs/开发文档.md`**：若描述涉及执行类别 / `spawn_blocking` 策略或线程模型，按 `.trae/rules/architecture-docs-sync.mdc` 同步。
- 本文件（`docs/design/README.md` §2 登记）。

---

## 风险与开放项

- **`from_std` 是否稳定保留 `process_group(0)`**：需回归用例核实；若不可靠，改为「spawn 前用 `pre_exec`（`unsafe`）显式 `setpgid(0,0)`」的备选路径。
- **`kill_on_drop` 与显式 kill 的关系**：避免重复 kill 造成的 `ESRCH` 噪声；约定显式 kill 后再置 `kill_on_drop` 无害化（或 kill 成功后由 reap 接管）。
- **同步 `run_tool*` 面的最终形态**：是彻底 async 化，还是保留「非 run_command 同步 + run_command 走 async 直连」——PR1 定稿并在 PR 描述记录。
- **`extra_stop` 是否升级为 `Future`**：PR1 先保留 `Fn` 轮询（`sleep` 退避），`Future`（SSE `Sender::closed()`）作为后续增强切片；需评估对 workflow 只传超时场景的接口影响。
- **Windows**：`from_std` 无 `process_group` 等价物；本次保持 Unix 优先，Windows 分支维持现状（`child.kill()` 兜底）。
