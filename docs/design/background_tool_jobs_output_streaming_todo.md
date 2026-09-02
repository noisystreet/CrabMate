# 后台工具任务实时输出流：实施计划（todo）

> **状态**：Proposed（待评审）。**受众**：维护 `cm_tools/subprocess_session`、`cm_internal/tool_jobs`、web 路由、`cm_config`、Client 后台任务气泡的开发者。  
> **依据**：决策见 [`background_tool_jobs_output_streaming.md`](./background_tool_jobs_output_streaming.md)（ADR）；字段级接口见 [`background_tool_jobs_output_streaming_contract.md`](./background_tool_jobs_output_streaming_contract.md)（**实现照此编码**）。  
> **跟踪**：落地后从本仓库 `docs/待办清单.md`（`tools/` 章「长耗时工具执行」分项，若新增了对应行）删除；本文件可改为修订记录或删节。

---

## 目标与非目标

**目标**：
- job 执行中的输出**实时可拉**：job 级有界环形缓冲（`seq` 单调）+ `GET /tools/jobs/{id}/output?cursor=` 增量轮询（`tail -f` 体验）。
- 超长输出（> `command_max_output_len`）全程可达缓冲（子进程层 `uncapped_live` opt-in）。
- 纯新端点/新配置/新可选字段；工具契约与 SSE 协议**零变化**；旧客户端零行为变化。

**非目标**：
- SSE `events` 事件端点（Phase 2，另立项）；全文历史回放（环形截断，`truncated` 语义）；多副本/持久化（沿用父 ADR 声明）；改 `run_command` 工具契约或 SSE 协议；`GET /tools/jobs/{id}` 状态端点行为变更。

---

## 前置与依赖

- **已就绪**：后台工具任务核心链路（#873/#874 已合入；#875/#67 待合入，不阻塞本切片——依赖的 `registry`/`worker`/`GET /tools/jobs/{id}` 均已存在）。
- **不阻塞**：Client（`crabmate-client`）改动走该仓流程；本仓先定契约与后端。
- **共享改造提示**：`subprocess_session` 的 `uncapped_live` 是共享代码路径，须保证默认 `false` 下同步路径零变化（回归测试随 1.1 落地）。

---

## PR 切片

### Slice 0：文档（ADR + 契约 + 本计划）提交

- [ ] 评审通过后提交三件套：`background_tool_jobs_output_streaming.md`（ADR）、`background_tool_jobs_output_streaming_contract.md`（契约）、`background_tool_jobs_output_streaming_todo.md`（本文件）。
- [ ] **提交方式（沿用父 ADR 先例）**：另开 `docs/tool-job-output-streaming-adr` 分支单独 PR，不并入功能 PR。

### Slice 1：后端核心

**1.1 子进程层 `uncapped_live`**（`src/cm_tools/subprocess_session.rs`）
- [ ] `SubprocessWaitCtl` 增 `uncapped_live: bool`（默认 `false`，`Default` 派生不变）。
- [ ] `append_captured`：`uncapped_live=true` 时无论 kept 捕获缓冲是否已满，都把读到的字节入 live 队列（kept 仍按 `max_capture_bytes` 前缀截断，只影响终态快照）。
- [ ] 回归：`uncapped_live=false`（默认）时既有同步路径 chunk 语义不变（单测钉：满 cap 后不再发 chunk）。

**1.2 job 环形输出缓冲**（`src/cm_internal/tool_jobs/`）
- [ ] `types.rs`：`JobOutputLog`（`seq` 单调自 1、`VecDeque<(u64 seq, SessionStream, String)>`、字节合计）；常量 `MAX_OUTPUT_ITEMS=8192`、`MAX_ITEMS_PER_RESPONSE=500`。
- [ ] `registry.rs`：侧表 `outputs: HashMap<String, JobOutputLog>`（**不并入 `JobRecord`**，避免状态轮询克隆缓冲）；`register` 建、TTL 清理/容量淘汰随记录删；方法 `push_output(id, stream, text)`（超字节/条数上限丢最旧）、`read_output(id, cursor) -> OutputRead { items, next_cursor, truncated, eof }`（eof 判定需 job 终态信息）、`complete()` 时终态裁剪（各流尾部 ≤ `command_max_output_len`）。
- [ ] `worker.rs`：`run_job_blocking` 增 `chunk_sink` 参数（内部置 `uncapped_live=true`），把 `(SessionStream, bytes)` 经 `take_utf8_text` lossy 后 `push_output`；sink 恒返回 `true`；`launch_job`/`enqueue_and_launch` 接线。

**1.3 配置**（`config/tools.toml` `[tool_registry]` + `cm_config`）
- [ ] 新增 `background_job_output_buffer_bytes`（默认 `262144`，范围 4096–16777216）；TOML 注释钉默认值；finalize 默认 + 钳制测试（对齐 `finalize_tests.rs` 风格）。
- [ ] 热重载：读取时机 = 创建 job 时；已运行 job 不受影响（随 1.2 落地并回归）。

**1.4 HTTP 端点**（`src/cm_api_contract/tool_jobs.rs` + `src/web/routes/tools/`）
- [ ] `GET /tools/jobs/{tool_job_id}/output`：契约 §2 响应字段（`ToolJobOutputResponseBody` / `ToolJobOutputItem`）+ `cursor` 解析（失败/负数 → 忽略按省略）。
- [ ] 错误码复用 `401/403/404/410`（`get_checked` 过期语义走既有路径；缓冲随记录同删）；归属校验同状态端点。
- [ ] OpenAPI：`openapi_paths_tool_jobs.rs` 增路径与 schema。

**1.5 观测与文档同步**（Slice 1 同 PR）
- [ ] `JobRegistryStats` 增 `output_bytes_total` / `output_dropped_events`；`/status` 暴露。
- [ ] `docs/命令行契约.md`：新端点；`docs/配置说明.md`：新配置键。（`docs/SSE协议.md` / `docs/工具说明.md` 无需改动。）

### Slice 2：Client（`crabmate-client` 仓，独立 PR）

- [ ] 后台任务气泡：`running` 时短轮询（~300–500 ms）`GET /tools/jobs/{id}/output?cursor=` 增量渲染输出（带 stdout/stderr 标记），`truncated=true` 显示"输出已截断"提示，`eof=true` 或终态后停止。
- [ ] 取消/终态摘要仍走既有端点与软字段（`tool_job_id` / `tool_job_poll_url`）；无新解析字段。
- [ ] `make frontend-check` 通过。

### Slice 3：可选增强（独立 PR，未承诺排期）

- [ ] SSE `GET /tools/jobs/{id}/events`（契约未含，另行 ADR）：重放环形缓冲 + per-job broadcast live；Last-Event-ID 断线续拉。
- [ ] （若产品要）后台任务历史列表 / 全文输出持久化（超出环形上界，另立项）。

---

## 测试计划

- **单测**（Slice 1）：
  - `uncapped_live=false` 回归：同步路径满 cap 后不再发 chunk；`=true`：超 `command_max_output_len` 的输出全量到达 sink。
  - 缓冲：seq 单调、按字节/条数上限丢最旧；落后游标 → `truncated` 重放；终态裁剪后仍可取尾部且 `eof` 正确；增量不重不漏（多轮 read 对拍）。
  - `eof`：非终态恒 `false`；终态空取/末批语义钉死一种并回归。
  - 并发：worker push + 轮询 read 交替（无丢失/乱序）；缓冲随记录 TTL/容量淘汰删除。
  - 端点：401/403（归属越权）/404/410；`cursor` 非法值回退。
  - 配置：默认值 + 钳制（4096–16777216）+ 热重载「创建时读取」。
- **回归**：`cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
- **e2e（可选）**：真实 `cargo build` async → `/output` 轮询滚动到 succeeded → 结尾错误文本可见 → `eof=true`。

## 完成定义（删对应待办条目前）

- `run_command async=true` 的 job 执行中，`GET /tools/jobs/{id}/output` 按契约 §2 返回增量输出；超长输出（> `command_max_output_len`）全程可见（非仅前缀）。
- 游标/截断/终态裁剪/`eof` 符合契约 §3；`GET /tools/jobs/{id}` 状态端点行为未变。
- 工具契约与 SSE 协议零变化，**未** bump `SSE_PROTOCOL_VERSION`；`uncapped_live=false` 同步路径回归通过。
- `docs/命令行契约.md` / OpenAPI / `docs/配置说明.md` 已同步；Client 侧同步或明示待办。

## 风险与开放问题

- **共享改造**：`uncapped_live` 触碰同步 `run_command` 的 chunk 链路，虽默认关闭仍须回归；实现时注意 flush 节奏（wait 循环间隔内 live 队列瞬时积压量 = 单间隔产出，可接受，不另加背压）。
- **历史有界**：环形裁剪使超上限早期输出不可回放（契约明示 `truncated`）；要全文需持久化另立项。
- **近似实时**：实时性受轮询间隔约束；毫秒级体验需 Phase 3 SSE。
- **Client 排期**：气泡实时输出在外部仓，需协调；后端先落地不影响默认行为。
- **`eof` 语义（已钉）**：`eof = 终态 && next_cursor > written`——末批非空即标 `eof=true`（契约 §3.2），已随实现回归。
- **近零时长任务竞态**：任务瞬间完成时 `complete()` 终态裁剪先于首次轮询发生，查看者只能拿到尾部——属既有"尾部保留"语义，非缺陷。
