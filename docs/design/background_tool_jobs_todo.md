# 后台工具任务：实施计划（todo）

> **状态**：Proposed（待评审 ADR 后启动）。**受众**：维护 `tool_registry`、`execute_run_command`、web 路由、`cm_sse_protocol`、Client `parser_v2` 的开发者。  
> **依据**：决策见 [`background_tool_jobs.md`](./background_tool_jobs.md)（ADR）；字段级接口见 [`background_tool_jobs_contract.md`](./background_tool_jobs_contract.md)（**实现照此编码**）。  
> **跟踪**：落地后从 **`docs/待办清单.md`**（`tools/` 章「长耗时工具执行」分项）删除对应内容；本文件可改为修订记录或删节。

---

## 目标与非目标

**目标**：
- `run_command` 支持可选 `async: true`：创建后台任务、立即返回启动 `tool_result`（`tool_job_id` / `tool_job_poll_url` / `tool_job_status`）。
- job 脱离当前 turn：`GET /tools/jobs/{id}` 轮询、`POST /tools/jobs/{id}/cancel` 取消；复用 `subprocess_session`（进程组 kill、截断、统计）。
- 默认关闭、全部软字段/新端点 → 旧客户端零行为变化。

**非目标**：
- 多副本/跨进程持久化（另立项）；job 结果自动回填模型上下文；`tool_job_finished` 默认不做（Phase 2 可选）；`run_command` 之外的工具先不开 async；不 bump `SSE_PROTOCOL_VERSION`。

---

## 前置与依赖

- **已就绪**：`subprocess_session`（可取消 + chunk + 观测）、P0/P1（#868/#869 已合并）、观测统计（PR #870 待合）。
- **不阻塞**：P2 按工具名墙钟（job 先用 `command_timeout_secs` + `timeout_secs`）；`run_and_format*` 迁移（只对 `run_command` 开 async）。
- **外部仓**：Client（`crabmate-client`）改动需走该仓流程；本仓先定契约与后端，Client 可并行或随后。

---

## PR 切片

### Slice 0：文档（ADR + 契约 + 本计划）提交

- [ ] 评审通过后提交：`background_tool_jobs.md`、`background_tool_jobs_contract.md`、`background_tool_jobs_todo.md`（本文件）+ `long_running_tool_execution_todo.md` P3 第 4 条链接（已改，未提交）。
- [ ] **提交方式（已定）**：另开 `docs/background-tool-jobs-adr` 分支单独 PR（PR #870 已合入，不可并入）。

### Slice 1：后端核心（`run_command` async + job 模块 + 端点）

**1.1 配置**（[`config/tools.toml`](../config/tools.toml) `[tool_registry]` + `cm_config`）
- [x] 新增 6 键（契约 §6）：`background_jobs_enabled=false`、`background_job_max_concurrent=4`、`background_job_max_queued=32`、`background_job_ttl_secs=86400`、`background_job_result_grace_secs=300`、`background_job_max_entries=128`；TOML 注释钉默认值。
- [x] 热重载：`POST /config/reload` 重建 `AgentConfig`（finalize 路径自动含新键默认值）；**「创建 job 时读取、已运行 job 不受影响」**的消费语义随 1.2/1.3 落地并回归。

**1.2 job 模块**（新目录 `src/cm_internal/tool_jobs/`，复用 `subprocess_session`）
- [x] `types.rs`：`JobStatus` 状态机（`queued/running/succeeded/failed/cancelled/timed_out`）、`JobRecord`（`tool_job_id`、`workspace`、来源 turn `job_id`、创建/完成时间、截断 stdout/stderr、`workspace_changed`）、原子状态转移（Mutex 临界区；`queued/running → cancelled`，已完成不可覆盖）。
- [x] `registry.rs`：`tool_job_id` 生成（`tooljob_` + 32 hex 随机，`getrandom`，不可枚举）；`Mutex<HashMap>` + 并发/排队上限 + 条目上限（**仅淘汰终态**，`queued`/`running` 不可淘汰）；TTL 清理定时器（创建起算 + 终态宽限 `result_grace_secs`，清理后轮询 410）。
- [x] `worker.rs`：`tokio::spawn_blocking` + `catch_unwind`（panic → **先 terminate 进程组**，再标 `failed`，`error_code=internal`）；`wait_child_session`（wall 默认 `command_timeout_secs`，`timeout_secs` 覆盖；取消走 `AtomicBool` → `Cancelled`）；成功结果可写 `test_result_cache`（缓存写入随 1.3 的 `run_command` 缓存键落地）；超时/取消不写、`workspace_changed=false`。
- [x] 启动 sweep：**内存注册表启动即为空，sweep 为空操作**（无持久化可清）；孤儿进程无法可靠识别（子进程无标记），不承诺清理，文档明示单副本不承诺崩溃恢复。

**1.3 `run_command` 集成**
- [x] `RunCommandArgs` 增 `#[serde(rename = "async")] pub async_: bool`（默认 false）与 `timeout_secs: Option<u64>`（钳制 1～600，对齐 `python_snippet_run`；**随本切片一并落地**，本属 P2 子项）；Schema 自动含新字段。
- [x] 门闩：`background_jobs_enabled=false` → `invalid_args`；需交互审批（AllowOnce）→ 拒绝。**async 仅对 `run_command` 开放**，不按命令/argv 分类禁（与 P2「不做 argv 启发式」一致）；并发写责任在 `docs/工具说明.md` 明示（1.5 同步）。
- [x] `execute_run_command.inc.rs` async 路径：白名单/路径/审批照旧（发起时刻，`async_mode` 拒绝一切交互审批）→ `enqueue_and_launch`（登记 + `try_start` 调度，并发满入队、完成后续领）→ 立即返回启动 `tool_result`（`tool_job_id` / `tool_job_poll_url` / `tool_job_status=queued`），`output` 为发起确认文案。
- [x] 启动帧软字段序列化 + 不 bump 协议（`tool_result.tool_job_*` 可选字段经注入 JSON → `ToolResultBody` 软字段；`append_tool_result_and_reflection` 跳过 `tool_job` 键不注入模型）。

**1.4 HTTP 端点**（`src/web/routes/` + `crabmate-api-contract`）
- [x] `GET /tools/jobs/{tool_job_id}`：契约 §3.1 响应字段 + 错误码 `401/403/404/410`（`JOB_NOT_FOUND` / `JOB_EXPIRED` / `JOB_OWNERSHIP_MISMATCH` 新增，同步 `crates/crabmate-api-contract/src/error_codes.rs`）。
- [x] `POST /tools/jobs/{tool_job_id}/cancel`：契约 §3.2（`queued` 不杀进程、完成态 409、幂等）。
- [x] 归属校验：随机 id 主防护；可选 `X-Workspace-Root` 头比对（不符 403）。

**1.5 文档同步**（Slice 1 同 PR）
- [x] `docs/SSE协议.md`：`tool_result` 软字段表 +（若未做 Phase 2 则注明 `tool_job_finished` 未实现）。
- [x] `docs/命令行契约.md` / OpenAPI：两个端点 + 新 `ApiError.code`。
- [x] `docs/工具说明.md`：`run_command` 的 `async` 参数与限制。
- [x] `docs/配置说明.md` / README：用户可见配置键。

### Slice 2：Client（`crabmate-client` 仓，可并行）

- [ ] `parser_v2.rs` / `sse_dispatch/types.rs`：`tool_result.tool_job_*` 软字段透传；参数表单加 `async`。
- [ ] 后台任务气泡：轮询 `GET /tools/jobs/{id}`（指数退避）+ 状态展示 + 取消按钮。
- [ ] 金样：`golden_ag_ui_v2_parser_matches_expected` + `make frontend-check`。

### Slice 3：可选增强（独立 PR，未承诺排期）

- [ ] SSE `tool_job_finished`（契约 §5）：`control_classify.rs` + 金样 + Client parser + `docs/SSE协议.md` 控制面一览。
- [ ] 观测扩展：job 级计数/时长日志（`tool_job_id`、来源 turn `job_id`、`duration_ms`）对接 `session_stats_snapshot`。
- [ ] （若产品要）后台任务 UI 增强：完成通知、历史列表。

---

## 测试计划

- **单测**（Slice 1）：job 生命周期转移；超时/取消杀进程组（复用 `subprocess_session` 测试模式）；**完成竞态**（cancel 不得覆盖 succeeded）；过期 → 410；认证/归属越权（403）；并发与排队上限（含 queued 取消不杀进程）；worker panic → `failed(internal)` 且进程组已终止；`timeout_secs` 钳制（1～600）；`deny_unknown_fields` 回归（旧服务端拒 `async`/`timeout_secs`）。
- **金样/双端**：本仓 `golden_ag_ui_classify_matches_expected`（若动分类）；Client `golden_ag_ui_v2_parser_matches_expected`。
- **e2e（可选）**：真实 `cargo build` async → 轮询到 succeeded → `workspace_changed` 语义。

## 完成定义（删对应待办条目前）

- `run_command` `async=true` 走后台：启动帧含 `tool_job_id`/`poll_url`，轮询/取消端点按契约返回；默认关闭时 `invalid_args`。
- 超时/取消/panic/过期路径符合契约 §4 状态机；不写缓存、不误标 `workspace_changed`。
- 全部新增为软字段/新端点/默认 false 参数，**未** bump `SSE_PROTOCOL_VERSION`；双端金样通过。
- `docs/SSE协议.md` / `docs/命令行契约.md` / `docs/工具说明.md` / `docs/配置说明.md` 已同步；Client 侧同步或明示待办。
- 白名单、路径、审批门闩回归未弱化。

## 风险与开放问题

- **崩溃不恢复**：serve 重启后 job 丢失（契约明示）；sweep 第一版只清注册表记录，孤儿进程不承诺清理。
- **并发写**：async 并行写 workspace 冲突责任在模型/调用方；不按命令分类禁（async 仅对 `run_command` 开放，`docs/工具说明.md` 明示）。
- **Client 排期**：后台气泡/轮询 UI 在外部仓，需协调；后端先落地不影响默认行为。
- **开放**：`tool_job_finished` 是否本期做（默认不做）。
