# ADR: 后台工具任务的实时输出流（background tool jobs output streaming）

> **状态**：Proposed（待评审）。**接口规格**（字段级，实现照此编码）：[`background_tool_jobs_output_streaming_contract.md`](./background_tool_jobs_output_streaming_contract.md)。**实施计划**：[`background_tool_jobs_output_streaming_todo.md`](./background_tool_jobs_output_streaming_todo.md)。**父决策**：[`background_tool_jobs.md`](./background_tool_jobs.md)（后台工具任务已合入：#873/#874）。**人读协议**：[`docs/命令行契约.md`](../命令行契约.md)。**版本轴**：[`client_contract_versioning.md`](./client_contract_versioning.md)。

## Context

- 后台工具任务（`run_command` 的 `async=true`）已脱离 turn 执行，状态轮询（`GET /tools/jobs/{id}`）与取消（`POST …/cancel`）已落地。**但执行中的输出不可见**：轮询只返回**终态、前缀截断**的 stdout/stderr；长构建（`cargo test` / `cmake --build` 数分钟）发起后只能看到"排队/运行中"状态卡，看不到输出滚动。
- 用户期望：发起长构建后能像 `tail -f` 一样**实时看到输出**（含结尾的编译错误），且不依赖任何 SSE 连接——job 本就与连接生命周期解耦。
- 已确认的实现事实（决定改动面）：
  - [`cm_tools/subprocess_session.rs`](../src/cm_tools/subprocess_session.rs) 有增量回调 `chunk_sink`，但 [`append_captured`](../src/cm_tools/subprocess_session.rs#L361) **只在截断缓冲未满时**把字节入 live 队列——输出超过 `command_max_output_len` 后，增量回调再也收不到字节。后台 worker 现传 `chunk_sink: None`。
  - sink 由 wait 循环在**调用方线程**同步 flush（单流内保序；双流按管道排空时序近似交错）。
  - 现有 `JobRecord` / `JobOutcome` 只存终态截断正文，不存执行过程；`registry` 为单 Mutex 临界区。
- 目标：新增 **job 级有界环形输出缓冲** + **增量轮询端点**，让调用方/用户轮询拉取运行中的输出；数据面设计为 seq 单调，预留 SSE（Phase 2）升级不改数据面。
- 约束：保持父 ADR「**轮询为主**」架构主线，不引入对 SSE 连接的依赖；工具契约、SSE 协议**零变化**（不 bump `SSE_PROTOCOL_VERSION`）；内存有界（受配置上限约束）；纯新端点/新配置/新可选字段，旧客户端零行为变化。

## Decision

### 1. 数据底座：job 级有界环形输出缓冲

- worker（`spawn_blocking` 内 `wait_child_session`）设置 `chunk_sink` → 把增量写入**注册表侧表**（`outputs: HashMap<tool_job_id, JobOutputLog>`，随 job 注册/清理同生命周期），**不放**进 `JobRecord`——避免状态端点克隆记录时把缓冲一并拷贝。
- 缓冲元素 = `{ seq, stream, text }`：
  - `seq`：**全局单调**（自 1 起，只在 append 处自增；裁剪只丢头部 → 保留段始终是连续区间）。
  - `stream`：`stdout` | `stderr`（与 `SessionStream::as_sse_label` 同值）。
  - `text`：按 `take_utf8_text` lossy 转 String（非法字节 U+FFFD，与同步 `tool_output_chunk` 一致）。
- **上界双保险**：字节上限 `background_job_output_buffer_bytes`（默认 256 KiB）+ 元素条数上限（硬编码 `8192`，防海量微块 → 每条元素平均开销上限约 32 B）；超限**丢最旧**。`chunk_sink` 恒返回 `true`（缓冲在环形层有界，无需背压）。
- **终态裁剪**：`registry.complete()` 时把缓冲按**合并时序**裁剪，尽力使 stdout、stderr 各留尾部 ≤ `command_max_output_len`（内存优先：末尾单条不拆、无法双达时可能丢尽他流，至少保留 1 条兜底）——内存界收敛为「并发 × 缓冲上限 + 终态条目 × ≤2×max_output_len（+单条）」；晚到的查看者仍能拉到**最终尾部**（恰是状态端点前缀截断给不到的部分，两者互补）。
- 记忆语义：缓冲是**环形尾部**而非全文；serve 重启即失（与父 ADR「崩溃不恢复」一致）。

### 2. 子进程层小改造：`uncapped_live`（opt-in，不改同步路径）

- `SubprocessWaitCtl` 增字段 **`uncapped_live: bool`**（默认 `false`）。为 `true` 时 `append_captured` **无论 kept 捕获缓冲是否已满**都把读到的字节入 live 队列；`max_capture_bytes` 仍只约束终态快照，**不再截断实时流**。
- 后台 worker（`run_job_blocking`）置 `true`；同步 `run_command` 等既有路径保持 `false` → **行为零变化**（现同步路径的输出超限即停发 chunk 的语义保留）。
- 这是共享会话层的扩展点：未来同步工具要"全量流"可直接复用。

### 3. 读取通道：增量轮询（主，本稿范围）

新端点 **`GET /tools/jobs/{tool_job_id}/output?cursor=<u64>`**：

- 返回自 `cursor`（含）起的保留元素；省略 `cursor` → 从最早可用起。响应 `{ tool_job_id, status, cursor, items[], truncated, eof }`。
- `cursor` 落后于已被丢弃的最早元素 → `truncated=true`，从最早可用重放（查看者失去间隙，UI 显示"输出被截断"）。
- 单响应条数上限（实现钉死 500，防大 JSON）；`eof = 终态 && 缓冲已无更多`（含裁剪后尾部已耗尽）→ 查看者可停止。
- 鉴权与归属复用状态端点语义（Bearer + 可选 `X-Workspace-Root`；`404 JOB_NOT_FOUND` / `410 JOB_EXPIRED`）。
- Client：「后台任务气泡」在 `running` 时把轮询间隔降到 ~300–500 ms 即近似实时（`tail -f` 体感）；终态退避照旧。

### 4. SSE 事件端点（Phase 2，本稿不做）

同一缓冲数据面可支撑 `GET /tools/jobs/{id}/events`（EventSource：先重放缓冲 `?cursor`/Last-Event-ID，再经 per-job broadcast 订阅 live）——`seq` 单调天然支持断线续拉。仅当产品要求**毫秒级**推送才立项，届时另开 ADR；本稿不承诺接口。

### 5. 配置与默认

`config/tools.toml`（`[tool_registry]`）新增 1 键：

```toml
# 每 job 环形输出缓冲字节上限（4096–16777216，默认 262144=256 KiB）；超限丢最旧，终态裁剪为尾部 ≤ 输出上限
# background_job_output_buffer_bytes = 262144
```

- 读取时机 = **创建 job 时**（与既有键「创建时读取、已运行不受影响」语义一致，热重载回归同款）。
- 无独立总开关：缓冲随 job 存在即启用（成本仅内存，受上界约束；`background_jobs_enabled=false` 时根本不会创建 job）。

### 6. 兼容与版本

- **工具契约零变化**：`run_command` 的 `async` 参数 schema 不动；本功能只在 job 生命周期内**追加一条 HTTP 读取通道**。
- SSE 协议零变化，**不 bump `SSE_PROTOCOL_VERSION`**。
- 新增仅：1 个 HTTP 端点 + 1 个可选查询参数 + 1 个配置键 + `SubprocessWaitCtl.uncapped_live`（默认 false）。新服务端 + 旧客户端同版本下一切照旧。
- HTTP 新端点同步 `docs/命令行契约.md` / OpenAPI（`crabmate-api-contract`）。

### 7. 观测

- 注册表统计（`JobRegistryStats` / `/status`）增：`output_bytes_total`（缓冲累计写入字节）、`output_dropped_events`（环形裁剪丢弃条数）。
- 日志带 `tool_job_id`（不打 argv，脱敏口径不变）。

## Consequences

**好处**：
- 长构建实时可见（输出滚动），编译错误在结尾的任务也能在结束后拉到**尾部**输出——补上状态端点前缀截断的盲区。
- 纯 HTTP 增量轮询，与父 ADR「轮询为主」一致；不占 SSE 连接，多客户端/断线重连天然友好。
- 数据面（seq 单调环形缓冲）对未来 SSE 升级无返工。

**代价与约束**：
- **近似实时**：实时性受轮询间隔约束（毫秒级体验需 Phase 2 SSE）。
- **历史有界**：超出环形缓冲上限的早期输出不可回放（`truncated` 语义）；要全文历史需另立项持久化。
- **内存有界但有量**：并发 job 各持 ≤ 缓冲上限，终态条目在 TTL+宽限内各多持 ≤ 2×max_output_len 尾部。
- **共享改造**：`subprocess_session` 的 `uncapped_live` 是共享代码路径的新增字段（默认 false）；回归面需覆盖同步路径零变化。
- 双流交错顺序为**近似**（按管道排空时序），不承诺跨流严格时序——展示时带 `stream` 标记。
- 行为/文档同步：`docs/命令行契约.md`、OpenAPI、`docs/配置说明.md`（新配置键）、Client 气泡 UI。

## Alternatives Considered

- **SSE/EventSource 作为唯一主通道**：否决。与父 ADR「轮询为主」相悖；需订阅生命周期、Bearer 认证、Last-Event-ID 重连状态机；"看进度"场景 500 ms 轮询已足够，SSE 留作 Phase 2 增量。
- **服务端记录全文、TTL 内完整可回放**：否决。内存不可控，chatty job（`cargo build -v`）分钟级可达数十 MB；环形尾部 + truncated 语义是正确取舍。
- **复用 `GET /tools/jobs/{id}` 返回"增量快照"**：否决。快照语义与增量游标混在一个响应里职责不清，且两者截断界不同（前缀 vs 尾部）；独立端点更清晰。
- **输出落盘 + 静态文件端点**：否决。污染工作区、鉴权/过期清理复杂，偏离内存注册表单副本模型。
- **把环形缓冲塞进 `JobRecord`**：否决。状态端点每次 `get()`/`get_checked()` 都克隆整条记录，256 KiB 缓冲随轮询反复拷贝；独立侧表隔离成本。

## 落地切片（评审后执行）

1. 本文档评审 → 同步 `docs/命令行契约.md` / OpenAPI / `docs/配置说明.md`（如涉 Client：其仓另行）。
2. 后端：`uncapped_live` + job 环形缓冲 + 输出端点 + 配置 + 观测；测试含游标/截断/终态裁剪/eof/越权/过期。
3. Client：气泡运行中短轮询展示输出 + 截断提示；终态仍走既有端点。
4. Phase 2（另立项）：SSE `events` 端点。
