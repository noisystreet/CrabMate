# 后台工具任务实时输出流：字段级接口规格（Contract）

> **状态**：Proposed（随 [ADR：后台工具任务的实时输出流](./background_tool_jobs_output_streaming.md) 一起评审）。本文件是**可执行契约**：实现切片照此编码，双端对齐照此检查。**父契约**：[`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)（状态/取消端点与状态机）。**人读协议**：[`docs/命令行契约.md`](../命令行契约.md)。**版本轴**：[`client_contract_versioning.md`](./client_contract_versioning.md)。

---

## 1. 数据面：job 输出缓冲

实现于注册表**侧表**（`outputs: HashMap<tool_job_id, JobOutputLog>`，随 job 注册创建、TTL/容量清理同生命周期删除；**不**并入 `JobRecord`）。

### 1.1 元素

| 字段 | 类型 | 说明 |
|------|------|------|
| `seq` | u64 | **全局单调**，自 1 起，append 处自增；裁剪只丢头部 → 保留段始终为连续区间 |
| `stream` | string | `stdout` \| `stderr`（与 `SessionStream::as_sse_label` 同值） |
| `text` | string | lossy UTF-8（非法字节 U+FFFD，复用 `take_utf8_text`，与同步 `tool_output_chunk` 一致） |

### 1.2 上界与裁剪

| 常量 | 值 | 说明 |
|------|-----|------|
| 字节上限 | `background_job_output_buffer_bytes`（配置，默认 `262144`） | 按元素文本字节合计；超限丢最旧（先凑到 ≤ 上限为止） |
| 元素条数上限 | `8192`（硬编码，非配置） | 防海量微块 → 元素平均开销上限约 32 B |
| 终态裁剪 | 各流尾部 ≤ `command_max_output_len`（**内存优先、尽力**） | `complete()` 时执行：合并时序下把 stdout、stderr 各自尾部压到 ≤ 该值（不拆元素）。末尾单条超大元素允许该流单独越界；无法同时达标时可能把另一流一并丢弃，**至少保留 1 条**兜底 |

- 裁剪语义 = 只影响「可重放的历史」；`seq` 编号不回填，保留段仍连续。
- 写入路径：worker `wait_child_session` 的 `chunk_sink` 回调（调用方线程同步 flush，单流内保序；双流按管道排空时序**近似**交错）。回调恒返回 `true`（无背压）。
- **前置改造**：`SubprocessWaitCtl` 增 `uncapped_live: bool`（默认 `false`）。后台 worker 置 `true`：`append_captured` 无论 kept 捕获缓冲是否已满都把读到的字节入 live 队列（详见 [ADR §2](./background_tool_jobs_output_streaming.md)）。同步路径保持 `false` → 行为零变化。

---

## 2. HTTP 端点：`GET /tools/jobs/{tool_job_id}/output`

鉴权与归属校验**完全复用**状态端点（Bearer 中间件 + 可选 `X-Workspace-Root` 头，不符 `403`；id 随机不透明为能力凭证）。

### 2.1 请求

| 参数 | 位置 | 类型 | 必填 | 说明 |
|------|------|------|------|------|
| `tool_job_id` | path | string | 是 | `tooljob_<32hex>` |
| `cursor` | query | u64 | 否 | 上次响应返回的 `cursor`；省略 → 从最早可用起。解析失败 / 为负 → 忽略按省略处理（宁从头，不可错序） |

### 2.2 200 响应

```json
{
  "tool_job_id": "tooljob_0123456789abcdef0123456789abcdef",
  "status": "running",
  "cursor": 42,
  "truncated": false,
  "eof": false,
  "items": [
    { "seq": 39, "stream": "stdout", "text": "   Compiling foo v0.1.0\n" },
    { "seq": 40, "stream": "stderr", "text": "warning: unused variable `x`\n" }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `tool_job_id` | string | 与路径一致 |
| `status` | string | 读取时刻快照：`queued` \| `running` \| `succeeded` \| `failed` \| `cancelled` \| `timed_out`（与状态端点同源） |
| `cursor` | u64 | **下次请求应携带的值**。= 最后一条 `item.seq` + 1；本次无 item 时 = 本次起点（省略请求时从最早可用起：有缓冲 = 最早保留 seq，空缓冲 = `1`，即下一条将是 `1`） |
| `items` | array | 自请求 `cursor`（含）起的保留元素，按 `seq` 升序；**跨响应不重不漏**（除非本响应 `truncated=true`）。单响应最多 500 条（实现钉死），超出部分下轮取 |
| `truncated` | bool | `true` = 请求 `cursor` 早于缓冲最早保留 seq，本次已从最早可用重放（中间有数据丢失，UI 须提示） |
| `eof` | bool | `true` = 任务已终态 **且** 本次已把缓冲（含终态裁剪后的尾部）全部返回 → 查看者应停止轮询 |

字段映射：`src/cm_api_contract/tool_jobs.rs` 增 `ToolJobOutputResponseBody` / `ToolJobOutputItem`（`schemars::JsonSchema` + `Serialize`）。

### 2.3 错误码（复用，无新增）

| HTTP | `ApiError.code` | 场景 |
|------|-----------------|------|
| 401 | `UNAUTHORIZED`（沿用认证中间件） | 未认证 |
| 403 | `JOB_OWNERSHIP_MISMATCH` | 提供 `X-Workspace-Root` 且与 job 记录不符 |
| 404 | `JOB_NOT_FOUND` | id 不存在 / 从未创建 |
| 410 | `JOB_EXPIRED` | 已过 TTL+宽限被清理（缓冲随记录一并删除） |

---

## 3. 游标语义与边界

### 3.1 三种读取窗口

设缓冲保留区间为 `[earliest, total]`（`total` = 已写入元素数；`earliest` = 最旧保留 seq；缓冲为空时视为 `[0,0]`）：

| 请求 `cursor` | 行为 |
|---------------|------|
| 省略（默认从最早） | 从 `earliest` 起返回（若缓冲为空，`items=[]`） |
| `earliest ≤ cursor ≤ total` | 正常增量：返回 `seq ≥ cursor` 的元素，**不重不漏** |
| `cursor < earliest` | `truncated=true`，从 `earliest` 重放（期间丢弃的旧数据不可恢复） |

### 3.2 `eof` 判定

**实现钉死一种语义**：`eof = status.is_terminal() && next_cursor > written`（`written` = 缓冲已写元素总数；含 `complete()` 时终态裁剪后的尾部）。

- 即：终态且本次响应已把缓冲（含裁剪后的尾部）**全部返回**——末批非空时本响应即标 `eof=true`（`cursor` 已越过末条），无需再轮询一次。
- 非终态（`queued`/`running`）恒 `eof=false`。
- 终态但缓冲为空（如 `spawn_failed` 无任何输出）：`next_cursor=1 > written=0` → `eof=true`、`items=[]`。

### 3.3 与状态端点 `GET /tools/jobs/{id}` 的关系

- 状态端点**不变**：终态 `stdout`/`stderr` 仍为既有**前缀截断**快照。
- 本端点提供**环形尾部**（含终态裁剪后的尾部）→ 两者互补；要"结尾的编译错误"看 `output`。
- 两端点读取互不干扰（缓冲在侧表，不随 `JobRecord` 克隆）。

### 3.4 时序示例（Client 视角）

```
Poll 1  GET /output                 → items[1..3] cursor=4 truncated=false eof=false   (running)
Poll 2  GET /output?cursor=4        → items[4..5] cursor=6 truncated=false eof=false   (running)
   … 缓冲超限丢 1、2（earliest=3）…
Poll N  GET /output?cursor=4        → truncated=true, items[3..5] cursor=6              (running)
Poll M  GET /output?cursor=10       → items=[] eof=true  (succeeded)
```

---

## 4. 配置键（`config/tools.toml` `[tool_registry]`）

| 键 | 类型 | 默认 | 范围 | 说明 |
|----|------|------|------|------|
| `background_job_output_buffer_bytes` | int | `262144` | 4096–16777216 | 每 job 环形输出缓冲字节上限；超限丢最旧；终态裁剪为各流尾部 ≤ `command_max_output_len` |

- Rust 字段：`ToolRegistryPolicyConfig.tool_registry_background_job_output_buffer_bytes`（`cm_config` finalize 默认值 + 范围钳制，测试对齐 `finalize_tests.rs` 风格）。
- 热重载：读取时机 = **创建 job 时**；已运行 job 不受后续变更影响（与既有 6 键同款消费语义）。
- 无独立开关：`background_jobs_enabled=false` 时不会创建 job，缓冲不产生。

---

## 5. 兼容窗口与双端对齐清单

- **工具契约零变化**：`run_command` 的 `async` / `timeout_secs` schema 不动；无新工具参数。
- **SSE 协议零变化**：不新增顶层键，**不 bump `SSE_PROTOCOL_VERSION`**。
- 新增仅：1 个 HTTP 端点（`/tools/jobs/{tool_job_id}/output`）+ 1 个可选查询参数 + 1 个配置键 + `SubprocessWaitCtl.uncapped_live`（默认 `false`）。旧服务端/旧客户端同版本互操作全部照旧。
- 文档同步：
  - [ ] `docs/命令行契约.md`：新端点（请求/响应/错误码表）。
  - [ ] OpenAPI：`openapi_paths_tool_jobs.rs`（`crabmate-api-contract`）增路径与 schema。
  - [ ] `docs/配置说明.md`：新配置键。
  - [ ] `docs/SSE协议.md` / `docs/工具说明.md`：**无需改动**（无 SSE/工具契约变化；如提一句 Client 行为可放 Client 仓）。

---

## 6. 实现落点映射

| 面 | 后端 | Client |
|----|------|--------|
| 子进程层 | `SubprocessWaitCtl.uncapped_live` + `append_captured`（`cm_tools/subprocess_session.rs`） | — |
| job 缓冲 | `src/cm_internal/tool_jobs/`：`types.rs`（`JobOutputLog`/元素）、`registry.rs`（侧表 `outputs` + `push_output`/`read_output(cursor)`/终态裁剪）、`worker.rs`（`chunk_sink` 接线，`run_job_blocking` 置 `uncapped_live`） | — |
| 配置 | `cm_config` `ToolRegistryPolicyConfig` + `config/tools.toml` | — |
| HTTP 端点 | `src/cm_api_contract/tool_jobs.rs`（响应体）+ `src/web/routes/tools/`（handler，Bearer + 归属校验 + `get_checked` 过期语义） | — |
| 观测 | `JobRegistryStats` 增 `output_bytes_total` / `output_dropped_events`；日志带 `tool_job_id` | — |
| UI | — | 后台任务气泡：`running` 时短轮询（~300–500 ms）拉 `/output` 增量渲染 + `truncated` 提示 + `eof` 后停止；取消/终态仍走既有端点 |

测试：游标增量不重不漏；环形裁剪 + 落后游标 → `truncated` 重放；终态裁剪后仍可取尾部且 `eof` 正确；超长输出（> `command_max_output_len`）经 `uncapped_live` 全量可达缓冲；`uncapped_live=false` 同步路径回归零变化；401/403/404/410；缓冲与记录同生命周期清理；并发读写（worker push + 轮询 read）无丢失/乱序；配置钳制（4096–16777216）。
