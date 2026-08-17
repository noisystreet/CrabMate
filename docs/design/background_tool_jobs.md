# ADR: 后台工具任务（background tool jobs）

> **状态**：Proposed（待评审）。**接口规格**（字段级，实现照此编码）：[`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)。**实施计划**：[`background_tool_jobs_todo.md`](./background_tool_jobs_todo.md)。**关联**：[`long_running_tool_execution_todo.md`](./long_running_tool_execution_todo.md)（P3 第 4 条）、[`tool_calling_evolution.md`](./tool_calling_evolution.md)（「长任务进度事件」）。**人读协议**：[`docs/SSE协议.md`](../SSE协议.md)、[`docs/命令行契约.md`](../命令行契约.md)。**版本轴**：[`client_contract_versioning.md`](./client_contract_versioning.md)。**双端对齐**：`.cursor/rules/api-sse-chat-protocol.mdc`。

## Context

- 长构建（`cargo test` / `pytest_run` / `cmake --build` 数分钟）目前**绑死当前 LLM turn**：SSE 连接必须保持、`RunLoopIo.cancel` 一关 job 就丢。模型只能干等，页面关掉则执行中止。
- 已落地底座：[`cm_tools/subprocess_session.rs`](../src/cm_tools/subprocess_session.rs) 已支持可取消会话（进程组 SIGTERM→SIGKILL、并发排空、`tool_output_chunk`、观测统计）；`run_command` 已 emit chunk；P0/P1（#868/#869）已合并。
- 目标：工具可**脱离当前 turn** 继续执行，调用方在**连接关闭后**仍能查询/取消；结果不自动塞回模型上下文（避免污染）。
- 约束：全部新增契约对旧客户端**可忽略**（软字段原则），不 bump `SSE_PROTOCOL_VERSION`；执行与回收都必须与 SSE 连接生命周期解耦。

## Decision

### 1. 触发方式：工具可选参数 `async`

具名工具（先 `run_command`；其余在迁入共享会话后开放）增加可选参数 **`async: bool`**（默认 `false`，行为不变）。

- `async=true` 时：白名单、`..`/绝对路径校验、交互审批**在发起时刻照常执行**；通过后创建 job，**立即**返回 `tool_result`（不阻塞、不执行），体内带 `tool_job_id` 与轮询地址。
- 不支持 async 的工具收到 `async=true`：`invalid_args`。
- 需要**交互审批**（`AllowOnce` 语义）的命令拒绝 async（job 执行期无法再次弹审批）；`AllowAlways` / 已在白名单者可用。

### 2. job 标识与生命周期

- **`tool_job_id`**（string，形如 `tooljob_<32hex>`，**随机不透明**，不可枚举）：**独立命名空间**，与 LLM turn 的 `job_id`（`x-stream-job-id` / `sse_capabilities.job_id`）**明确区分**，避免日志/端点歧义。job 记录绑定**来源会话 + workspace**，轮询/取消端点校验归属，防越权读取。
- 状态机：`queued → running → succeeded | failed | cancelled | timed_out | expired`。
- 超时：默认 `command_timeout_secs`；工具参数 `timeout_secs` 显式覆盖（钳制同现有 1～600）。取消：复用 `subprocess_session` 的进程组 kill（`Cancelled` 路径已实现）。
- 输出：轮询响应返回已截断 stdout/stderr（复用 `command_max_output_len` + 行数上限）；成功结果**可**写入 `test_result_cache`；超时/取消**不**写、不把 `workspace_changed` 置 true（与 P0 约束一致）。

### 3. 结果回收：轮询为主、事件为辅

- **主通道 = HTTP 轮询**（不依赖连接生命周期）：
  - `GET /tools/jobs/{tool_job_id}` → JSON：`{ status, exit_code?, stdout?, stderr?, summary?, error_code?, workspace_changed?, result_version? }`（`succeeded`/`failed` 含最终正文形状，与 `tool_result` 同源）。
  - `POST /tools/jobs/{tool_job_id}/cancel` → `{ status }`。**仅对 `queued`/`running` 生效**（原子状态转移）；job 已完成时返回当前状态（`409` 或直接回显，实现时钉死一种）。
  - 已过期被清理的 job：返回 **`410 Gone`**（`expired` 不保留条目，到期即删除）。
  - 鉴权复用现有 protected routes（Bearer）；**归属校验**：读取与取消均校验 `tool_job_id` 绑定的来源会话/workspace 与调用方一致。
- **TTL 起算与宽限**：TTL 自**创建**算（对齐 `background_job_ttl_secs`），但结果完成后额外保留宽限 `background_job_result_grace_secs`（默认 300s），避免长 job"刚完成即被清"。
- **辅助 = 尽力而为 SSE 补发**（Phase 2，可选）：新顶层键 **`tool_job_finished`**（体含 `tool_job_id`、`status`、`exit_code`、`summary`）——**仅当原 SSE 连接仍存活**时投递，作为"顺路提醒"，不参与主流程；旧客户端忽略未知键。

### 4. 启动帧（SSE `tool_result`）

`async=true` 的启动 `tool_result`（`result_version` 不变）新增**可选**字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `tool_job_id` | string? | 后台任务 id（旧客户端忽略） |
| `tool_job_poll_url` | string? | `GET /tools/jobs/{id}` 相对路径（旧客户端忽略） |

`output` 为发起确认文案（如「已创建后台任务 tooljob-…，轮询 …」），**不是**执行结果。不新增 `tool_job_started` 顶层键（避免多余生命周期）。

### 5. 执行模型（脱离 turn）

- job worker **不持有** `TurnControlSink` / `RunLoopIo.cancel` / SSE sender；取消改为显式 `POST …/cancel`（置 job 级 `AtomicBool`，`wait_child_session` 走既有 `Cancelled` 路径）。
- 复用 `subprocess_session::wait_child_session`（wall + 可取消 + 截断缓冲 + 统计）作为 worker 执行层；`tokio::spawn_blocking` 包一层（与现 run_command 一致；迁 `tokio::process` 时随共享会话一并升级）。
- 注册表：进程内 `Mutex<HashMap<tool_job_id, JobState>>` + 过期清理（TTL 与条目上限）。**多副本**需外部代理/持久化（与 `chat_job_queue` 既有声明一致），另立项。
- **排队语义**：超过 `background_job_max_concurrent` 的 async 调用进入 `queued`（FIFO，排队上限 `background_job_max_queued`，超限拒绝）；`queued` 状态取消**不**走杀进程路径（直接标 `cancelled`）。
- **worker 异常兜底**：`spawn_blocking` 闭包包 `catch_unwind`；`JoinHandle` 出错/panic → 标 `failed`（`error_code=internal`），**不得**卡 `running` 直至 TTL。
- **启动清理**：serve 启动时 sweep 残留 job 状态。**单副本不承诺崩溃恢复**（内存注册表，进程死亡即丢，子进程成孤儿）：sweep 按可识别标记清理残留子进程组，无法可靠识别的在文档明示。
- **并发写约束**：async 允许并行 job，写盘类并发冲突责任在模型/调用方；已知写盘类命令默认禁 async（实现时按工具分类钉死，与 P0「写工具并行仍走串行批」对齐）。

### 6. 配置与默认

`config/tools.toml`（`[tool_registry]`）新增，**默认关闭**，避免未升级客户端看到意外行为：

```toml
# 后台工具任务总开关（默认 false；开启后模型仍须显式传 async=true）
# background_jobs_enabled = false
# background_job_max_concurrent = 4
# background_job_max_queued = 32
# background_job_ttl_secs = 86400
# background_job_result_grace_secs = 300
# background_job_max_entries = 128
```

### 7. 兼容与版本

- 全部新增为**软字段/新端点/默认 false 参数** → **不 bump `SSE_PROTOCOL_VERSION`**（对齐 `client_contract_versioning.md` §2.1/§3.1：新服务端 + 旧客户端同版本下忽略新键继续工作）。
- `tool_job_finished` 新顶层键若实现：按 `.cursor/rules/api-sse-chat-protocol.mdc` 同步 `control_classify.rs`、金样、Client `parser_v2.rs`；本仓跑 `golden_ag_ui_classify_matches_expected`，Client 跑 `golden_ag_ui_v2_parser_matches_expected`。
- HTTP 新端点同步 `docs/命令行契约.md` / OpenAPI。

### 8. 观测

- 复用 `session_stats_snapshot()` 与会话级日志；job 级新增：`started / succeeded / failed / cancelled / timed_out / expired` 计数与时长，日志带 `tool_job_id` 与来源 turn `job_id`（脱敏，不打 argv）。

## Consequences

**好处**：长构建不再占死 turn/连接；模型可"发起后继续聊"，调用方可随时查状态或取消；全部复用共享会话的杀进程/截断/统计能力。

**代价与约束**：
- 用户/模型必须**显式**轮询（事件是尽力而为）；体验上需 Client 提供"后台任务"气泡与轮询 UI。
- 交互审批类命令不能 async（发起点即审批点），`AllowOnce` 场景受限。
- 单进程注册表有内存上限与 TTL，长保留需另做持久化；多副本不支持（另立项）。
- **崩溃恢复不承诺**：serve 重启/宕机后 job 状态丢失，调用方得 `410`/`404`；仅启动 sweep 兜底孤儿进程，不能保证可靠回收。
- **取消是尽力而为**：仅对 `queued`/`running` 生效；与完成竞态时以原子状态转移为准（不会把成功覆盖成取消）。
- 行为变化需文档同步：`docs/工具说明.md`（`run_command` 的 `async`）、`docs/SSE协议.md`（`tool_result` 软字段、可选 `tool_job_finished`）、`docs/命令行契约.md` / OpenAPI（新端点）、`README.md`（若用户可见配置）。

## Alternatives Considered

- **SSE 订阅作为唯一回收机制**：否决。job 与连接解耦是核心约束；连接关闭后订阅即失效。轮询为主、事件为辅。
- **完成后回填对话（assistant 消息）**：否决。污染对话历史、需要模型参与，且 job 可能属于已结束会话。
- **独立 `start_job` / `job_poll` / `job_cancel` 工具组**：否决。模型需学新工具名，且审批/白名单/路径校验要在新工具上重复实现。
- **turn 保持打开直到 job 结束**：否决。正是要解决的问题本身。
- **bump `SSE_PROTOCOL_VERSION`**：否决。无破坏性变更，软字段即可；bump 会强制 Client 同步发版。

## 落地切片（评审后执行）

1. 本文档评审 → 同步 `docs/SSE协议.md` / `docs/命令行契约.md` / `docs/工具说明.md` / `config/tools.toml` 注释（若动 `control_classify`：金样）。
2. 后端：job 注册表 + worker + 两个端点 + `run_command` 的 `async` 参数 + 启动 `tool_result` 帧 + 配置；测试含生命周期/超时/取消（含完成竞态）/过期/认证/**归属越权**/并发与排队上限/**worker panic 兜底**。
3. Client：`tool_job_id` / `tool_job_poll_url` 软字段解析、后台任务气泡与轮询 UI、`tool_job_finished`（若实现）。
4. 观测接入 + 可选 `tool_job_finished` 补发；多副本持久化另立项。
