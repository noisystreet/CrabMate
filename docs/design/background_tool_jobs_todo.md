# 后台工具任务：实施计划（todo）

> **状态**：核心链路实施完成；#873/#874/#875 与 Client `crabmate-client#67` 均已合入；Slice 4（模型侧只读查询工具 P0）已落地；Slice 3 首项（SSE `tool_job_finished` 补发）已落地。**受众**：维护 `tool_registry`、`execute_run_command`、web 路由、`cm_sse_protocol`、Client `parser_v2` 的开发者。  
> **依据**：决策见 [`background_tool_jobs.md`](./background_tool_jobs.md)（ADR）；字段级接口见 [`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)（**实现照此编码**）。  
> **跟踪**：Slice 0–2、4 已完成并合入，逐条实现细节见 ADR 与代码；本文件保留为**修订记录 + 未完成项跟踪**。剩余待办（Slice 3）同步维护于 **`docs/待办清单.md`**（`tools/` 章「长耗时工具执行」分项）。

---

## 目标与非目标

**目标**：
- `run_command` 支持可选 `async: true`：创建后台任务、立即返回启动 `tool_result`（`tool_job_id` / `tool_job_poll_url` / `tool_job_status`）。
- job 脱离当前 turn：`GET /tools/jobs/{id}` 轮询、`POST /tools/jobs/{id}/cancel` 取消；复用 `subprocess_session`（进程组 kill、截断、统计）。
- 默认关闭、全部软字段/新端点 → 旧客户端零行为变化。

**非目标**：
- 多副本/跨进程持久化（另立项）；job 结果自动回填模型上下文；`run_command` 之外的工具先不开 async；不 bump `SSE_PROTOCOL_VERSION`。

---

## 修订记录（已完成切片）

### Slice 0：文档（ADR + 契约 + 本计划）提交
- [x] 提交 `background_tool_jobs.md`、`background_tool_jobs_contract.md`、本文件 + `long_running_tool_execution_todo.md` P3 第 4 条链接。
- [x] 另开 `docs/background-tool-jobs-adr` 分支单独 PR（PR #870 已合入）。

### Slice 1：后端核心（`run_command` async + job 模块 + 端点）— PR #873 / #874 已合入
- [x] **1.1 配置**：`[tool_registry]` 新增 6 键（`background_jobs_enabled=false`、`background_job_max_concurrent=4`、`background_job_max_queued=32`、`background_job_ttl_secs=86400`、`background_job_result_grace_secs=300`、`background_job_max_entries=128`）；热重载经 `POST /config/reload` 重建 `AgentConfig`，创建 job 时读取、已运行 job 不受影响。
- [x] **1.2 job 模块**（`src/cm_internal/tool_jobs/`，复用 `subprocess_session`）：`types.rs` 状态机（`queued/running/succeeded/failed/cancelled/timed_out`）+ 原子状态转移；`registry.rs` 不可枚举 `tool_job_id`（`tooljob_` + 32 hex）+ 并发/排队/条目上限（仅淘汰终态）+ TTL 清理（创建起算 + 终态宽限，清理后轮询 410）；`worker.rs` `spawn_blocking` + `catch_unwind`（panic 先 terminate 进程组再标 `failed(internal)`）；启动 sweep 为空操作（无持久化，不承诺崩溃恢复）。
- [x] **1.3 `run_command` 集成**：`RunCommandArgs` 增 `async`（默认 false）与 `timeout_secs`（钳制 1～600）；门闩 `background_jobs_enabled=false` → `invalid_args`，需交互审批 → 拒绝；async 仅对 `run_command` 开放；启动帧软字段序列化且不 bump 协议（`tool_job` 键不注入模型）。
- [x] **1.4 HTTP 端点**（`src/web/routes/` + `crabmate-api-contract`）：`GET /tools/jobs/{tool_job_id}`（错误码 `401/403/404/410`，新增 `JOB_NOT_FOUND` / `JOB_EXPIRED` / `JOB_OWNERSHIP_MISMATCH`）；`POST /tools/jobs/{tool_job_id}/cancel`（`queued` 不杀进程、完成态 409、幂等）；归属校验（随机 id 主防护 + 可选 `X-Workspace-Root` 头比对）。
- [x] **1.5 文档同步**：`docs/SSE协议.md`（`tool_result` 软字段表）、`docs/命令行契约.md` / OpenAPI（两端点 + 新 `ApiError.code`）、`docs/工具说明.md`（`async` 参数与限制）、`docs/配置说明.md` / README（用户可见配置键）。

### Slice 2：Client（`crabmate-client` 仓）— PR #67 已合入
- [x] `parser_v2.rs` / `sse_dispatch/types.rs`：`tool_result.tool_job_*` 软字段透传。
- [x] 后台任务气泡：轮询 `GET /tools/jobs/{id}`（指数退避）+ 状态展示 + 取消按钮。
- [x] 金样：`golden_ag_ui_v2_parser_matches_expected`（client 仓无 `fixtures/`，以契约反序列化 + metadata 透传单测补位）+ `make frontend-check`。

### Slice 4：模型侧只读查询工具（P0，已落地）
**背景**：`async: true` 早已可用，但模型**没有任何工具**能查 job（只能靠 HTTP 轮询），因此「后台启动」对模型实际不可用。本切片只解决「**取回结果**」。

- [x] host trait：`cm_tools/memory_tool_host.rs` 追加 `ToolJobsToolHost`（`status` / `list`）；`cm_internal/memory_tool_hosts.rs` 实现 `ToolJobsHost`（只读 registry）。
- [x] `ToolJobRegistry::list(workspace, limit)`：最新创建在前 + 截断；不做惰性过期（归 `get_checked` / `cleanup`）。
- [x] `ToolContext.tool_jobs_host` 软字段 + `tool_context_for_with_read_cache_and_memory` 第 8 参；`tool_context_for` 签名不变（填 `None`）。
- [x] 透传链：`SyncDefaultToolDispatchArgs.tool_jobs` → 内联分支与 `spawn_blocking` 分支各自注入 `jobs_host`。
- [x] 两工具注册：`background_job_status` / `background_job_list`（spec / runner / 参数 `deny_unknown_fields` + `schemars(range)` / 摘要 / `dev_tag` → `GENERAL` / `ToolCategory::Development`）。
- [x] 只读判定 + **豁免**同轮 `(name, args)` 去重缓存（`registry_policy::tool_output_dedup_cache_eligible` + `builtin_dedup_cache_exempt_tools`）。
- [x] `run_command` ToolSpec description 宣传 `async` / `timeout_secs` 与两个查询工具（纯文案）。
- [x] 文档：`docs/工具说明.md` / `docs/en/TOOLS.md`。
- [x] 测试：`ToolJobsHost` 单测（`NotFound` / `Queued` / `Succeeded` / `Failed` / 终态无 outcome / TTL 过期 / 截断 / 多字节边界 / list 空结果 / 工作区过滤 + 最新在前 + 命令摘要 / limit）与工具参数层单测；async 端到端（真实 `dispatch_tool` 闭环）。
- **未做（本切片范围外，理由见 ADR §9）**：工作区门闩特例放行；并行只读批注入 registry（现为宿主 `None` 降级文案）；`background_job_cancel` 之类**发起/变更型**工具（ADR Alternatives 明确否决）。

---

## 未完成项（Slice 3：可选增强，独立 PR，未承诺排期）

- [x] SSE `tool_job_finished`（契约 §5）：`control_classify.rs` + 金样 + `docs/SSE协议.md` 控制面一览。**已落地**：`SsePayload::ToolJobFinished` + `ToolJobFinishedBody`；`control_classify` 加键；金样加行；发起时捕获 `WebToolRuntime.out_tx` 存入注册表侧表，`complete` 时 `try_send` 尽力而为（连接关闭即丢）。**Client parser 侧仍待**（跨仓，旧客户端忽略未知键）。
- [ ] 观测扩展：job 级计数/时长日志（`tool_job_id`、来源 turn `job_id`、`duration_ms`）对接 `session_stats_snapshot`。
- [ ] （若产品要）后台任务 UI 增强：完成通知、历史列表。

---

## 测试计划

- **单测**（Slice 1）：job 生命周期转移；超时/取消杀进程组；**完成竞态**（cancel 不得覆盖 succeeded）；过期 → 410；认证/归属越权（403）；并发与排队上限；worker panic → `failed(internal)` 且进程组已终止；`timeout_secs` 钳制（1～600）；`deny_unknown_fields` 回归。
- **单测**（Slice 4）：`ToolJobsHost::status` / `truncate_text`（多字节边界）/ `ToolJobsHost::list` / 工具参数层（`deny_unknown_fields`、空 id、`limit` 钳制、宿主 `None` 降级文案）。
- **金样/双端**：本仓 `golden_ag_ui_classify_matches_expected`（若动分类）；Client `golden_ag_ui_v2_parser_matches_expected`。
- **e2e**：真实 `cargo build` async → 轮询到 succeeded → `workspace_changed` 语义；**已落地**：Slice 4 在真实 `dispatch_tool` 路径上跑「`run_command async:true` 发起 → `background_job_status` 取回终态输出 → `background_job_list` 列出」闭环（`cm_internal::tool_registry::tests`）。

## 完成定义（删对应待办条目前）

- `run_command` `async=true` 走后台：启动帧含 `tool_job_id`/`poll_url`，轮询/取消端点按契约返回；默认关闭时 `invalid_args`。
- **模型可自助取回**：`background_job_status` 能按 `tool_job_id` 取回终态输出，`background_job_list` 能列出本工作区任务；两工具只读、不发起执行、不参与同轮去重缓存。
- 超时/取消/panic/过期路径符合契约 §4 状态机；不写缓存、不误标 `workspace_changed`。
- 全部新增为软字段/新端点/默认 false 参数，**未** bump `SSE_PROTOCOL_VERSION`；双端金样通过。
- `docs/SSE协议.md` / `docs/命令行契约.md` / `docs/工具说明.md` / `docs/en/TOOLS.md` / `docs/配置说明.md` 已同步；Client 侧同步或明示待办。
- 白名单、路径、审批门闩回归未弱化。

## 风险与开放问题

- **崩溃不恢复**：serve 重启后 job 丢失（契约明示）；sweep 第一版只清注册表记录，孤儿进程不承诺清理。
- **并发写**：async 并行写 workspace 冲突责任在模型/调用方；不按命令分类禁（async 仅对 `run_command` 开放，`docs/工具说明.md` 明示）。
- **Client 排期**：后台气泡/轮询 UI 在外部仓，需协调；后端先落地不影响默认行为。
- **开放**：`tool_job_finished` 是否本期做（默认不做）。
