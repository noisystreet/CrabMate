# 后台工具任务：字段级接口规格（Contract）

> **状态**：Proposed（随 [ADR：后台工具任务](./background_tool_jobs.md) 一起评审）。本文件是**可执行契约**：实现切片照此编码，双端对齐照此检查。**人读协议**：[`docs/SSE协议.md`](../SSE协议.md)、[`docs/命令行契约.md`](../命令行契约.md)。**版本轴**：[`client_contract_versioning.md`](./client_contract_versioning.md)。**金样**：`.cursor/rules/api-sse-chat-protocol.mdc`。

---

## 1. 工具契约：`run_command` 可选参数 `async`

### 1.1 Schema（`RunCommandArgs`）

现有字段（`src/cm_tools/tools/tool_param_types/part_tool_params_exec_file.inc.rs`，`#[serde(deny_unknown_fields)]`）：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `command` | string | 是 | 现有语义不变 |
| `args` | string[] | 否 | 现有语义不变 |
| `timeout_secs` | int | 否 | **新增**；墙钟秒（钳制 1～600，对齐 `python_snippet_run`）；仅 async 与非 async 均生效。**随本功能在 Slice 1 一并落地**（本属 P2 子项） |
| `async` | bool | 否 | **新增**；默认 `false`（同步，现有行为不变） |

- Rust 侧 `async` 是关键字：结构体字段用 `#[serde(rename = "async")] pub async_: bool`（或等价 raw identifier），JSON Schema 输出名为 `async`。
- 备选命名 `run_async` / `background`：否决，保持与语义、文档一致的 `async`。
- **`deny_unknown_fields` 的兼容含义**：新客户端对旧服务端传 `async` / `timeout_secs` 会被旧服务端拒绝（unknown field）——兼容窗口只承诺「新服务端 + 旧客户端」，不承诺「新客户端 + 旧服务端」（见 §8）。

### 1.2 语义

| 条件 | 行为 |
|------|------|
| `async` 省略 / `false` | 现状串行执行，`tool_result` 为最终结果 |
| `async: true`，配置 `background_jobs_enabled=false` | `invalid_args`：后台任务未启用 |
| `async: true`，需**交互审批**（`AllowOnce` 语义 / 需弹审批） | 拒绝：`invalid_args`，提示先 `AllowAlways` 或去掉 async |
| `async: true`，白名单/路径校验通过 | 创建 job → **立即**返回启动 `tool_result`（§2），不执行 |

- 发起时刻即完成白名单、`..`/绝对路径校验与交互审批（`AllowAlways` / 已在白名单者放行）。
- **async 仅对 `run_command` 开放**（本切片范围）；不按命令/argv 分类禁 async（与 P2「不做 argv 启发式」一致）。**并发写 workspace 的冲突责任在模型/调用方**，在 `docs/工具说明.md` 明示。

---

## 2. 启动帧：`tool_result` 软字段

发起 `async=true` 时，`tool_result`（`result_version` 不变）新增**可选**字段（旧客户端忽略）：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `tool_job_id` | string | 否 | 后台任务 id：`tooljob_` + 32 hex（随机不透明，不可枚举） |
| `tool_job_poll_url` | string | 否 | 相对路径：`GET /tools/jobs/{tool_job_id}` |
| `tool_job_status` | string | 否 | 固定 `"queued"`（发起时尚未运行） |

- `output`：发起确认文案（如「已创建后台任务 `tooljob_…`，轮询 `GET /tools/jobs/{…}`」），**不是**执行结果。
- `exit_code` / `stdout` / `stderr` / `error_code`：本帧**省略**。
- 不再新增 `tool_job_started` 顶层 SSE 键（`tool_result` 已收束工具生命周期）。

---

## 3. HTTP 端点

鉴权：与受保护路由一致（Bearer，`web_api_require_bearer`）。新增端点需同步 `docs/命令行契约.md` / OpenAPI（`crabmate-api-contract`）。

### 3.1 `GET /tools/jobs/{tool_job_id}`

轮询任务状态与（终态）输出。

**200** JSON（字段与 `tool_result` 同源，`status` 为任务状态）：

```json
{
  "tool_job_id": "tooljob_0123456789abcdef0123456789abcdef",
  "status": "succeeded",
  "exit_code": 0,
  "stdout": "…（截断）",
  "stderr": "…（截断）",
  "summary": "…",
  "error_code": null,
  "failure_category": null,
  "workspace_changed": true,
  "result_version": 1
}
```

| 字段 | 类型 | 非终态时 | 说明 |
|------|------|---------|------|
| `tool_job_id` | string | 有 | 与路径一致 |
| `status` | string | 有 | `queued` \| `running` \| `succeeded` \| `failed` \| `cancelled` \| `timed_out` |
| `exit_code` | int? | 无 | 终态才有；`timed_out`/`cancelled` 为 `null`（复用 -1 语义亦可，实现定一种） |
| `stdout` / `stderr` | string? | 有（增量快照） | 复用 `command_max_output_len` + 行数上限截断；每次返回**当前快照**，不累积 |
| `summary` | string? | 无 | 终态摘要（与 `tool_result.summary` 同源） |
| `error_code` | string? | 无 | 终态失败时的 `tool_result.error_code` 词汇（§8） |
| `failure_category` | string? | 无 | 与 `ToolFailureCategory::as_str` 一致 |
| `workspace_changed` | bool | 无 | job 结束后的最终值；超时/取消恒为 `false`（与 P0 约束一致） |
| `result_version` | int | 有 | 与 `tool_result.result_version` 对齐（当前 1） |

**错误码**：

| HTTP | `ApiError.code` | 场景 |
|------|-----------------|------|
| 401 | `UNAUTHORIZED`（沿用现有认证中间件码） | 未认证 |
| 403 | `JOB_OWNERSHIP_MISMATCH` | 提供 `X-Workspace-Root` 且与 job 记录不符（§3.3） |
| 404 | `JOB_NOT_FOUND` | id 不存在 / 从未创建 |
| 410 | `JOB_EXPIRED` | 已过 TTL+宽限被清理 |

### 3.2 `POST /tools/jobs/{tool_job_id}/cancel`

**200** JSON：`{ "tool_job_id": "…", "status": "cancelled" }`。

| 场景 | 行为 |
|------|------|
| `queued` | 直接标 `cancelled`（**不**走杀进程路径） |
| `running` | 置 job 级取消 → `subprocess_session` 走 `Cancelled`（进程组 SIGTERM→SIGKILL）→ `cancelled` |
| 已是 `cancelled` | 幂等：返回 **200** 当前状态 |
| 其它终态（`succeeded`/`failed`/`timed_out`） | 不覆盖：返回 **409** `{ "status": "<当前状态>" }`（原子状态转移，杜绝把成功覆盖成取消） |
| 不存在 / 已过期 | 404 / 410（同 §3.1） |

### 3.3 归属校验

- **主防护**：`tool_job_id` 为随机不透明值（32 hex，`getrandom`/`rand` 生成），不可枚举 → 知晓 id 即能力凭证（capability URL）。
- **增强（可选实现）**：job 记录保存创建时的 `workspace`；请求带 `X-Workspace-Root` 头时按规范化路径比对，不符返回 **403**。不带头则仅凭 id（单用户默认部署等价）。

---

## 4. 状态机与转移

```
created(queued) ──worker 领取──▶ running ──exit 0──▶ succeeded
                                  ├──exit ≠0──────▶ failed
                                  ├──墙钟到期──────▶ timed_out
                                  └──cancel 置位───▶ cancelled
queued ──cancel──▶ cancelled
任意终态 ──TTL+宽限过期──▶ 删除（轮询 410）
```

| 转移 | 触发 | 要点 |
|------|------|------|
| `queued → running` | worker 领取（并发上限内） | FIFO |
| `running → succeeded` | 子进程 exit 0 | 可写 `test_result_cache` |
| `running → failed` | exit ≠0 / spawn 失败 / **worker panic**（`catch_unwind` → 先 terminate 进程组，再标 `error_code=internal`） | 不得卡 `running` 直至 TTL |
| `running → timed_out` | 墙钟到期（默认 `command_timeout_secs`，可 `timeout_secs` 覆盖） | 不缓存、`workspace_changed=false` |
| `queued/running → cancelled` | §3.2 | 原子转移，已完成不可取消 |
| 任意 → 删除 | TTL（创建起算）+ 完成后宽限 | 清理定时器扫描 |

---

## 5. SSE `tool_job_finished`（Phase 2，可选）

仅当**原 SSE 连接仍存活**时尽力而为补发（连接关闭不发，主通道是轮询）。旧客户端忽略未知顶层键。

| 顶层键 / 形状 | 字段 | 说明 |
|---------------|------|------|
| `tool_job_finished` | `tool_job_id`（string，必填） | 与启动帧一致 |
| | `status`（string，必填） | `succeeded` \| `failed` \| `cancelled` \| `timed_out` |
| | `exit_code`（int?） | 终态退出码 |
| | `summary`（string?） | 终态摘要 |
| | `error_code`（string?） | 失败词汇（§8） |

实现此键时按 `.cursor/rules/api-sse-chat-protocol.mdc` 同步 `control_classify.rs` + 金样 + Client `parser_v2.rs`，本仓跑 `golden_ag_ui_classify_matches_expected`。

---

## 6. 配置键（`config/tools.toml` `[tool_registry]`）

| 键 | 类型 | 默认 | 说明 |
|----|------|------|------|
| `background_jobs_enabled` | bool | `false` | 总开关；关闭时 `async=true` 返回 `invalid_args` |
| `background_job_max_concurrent` | int | `4` | 同时运行上限；超出进入 `queued` |
| `background_job_max_queued` | int | `32` | 排队上限；超限拒绝创建 |
| `background_job_ttl_secs` | int | `86400` | 自**创建**起算的保留时长 |
| `background_job_result_grace_secs` | int | `300` | 终态后再保留的宽限（避免"刚完成即被清"） |
| `background_job_max_entries` | int | `128` | 注册表条目上限；**仅淘汰终态**条目（`queued`/`running` 不可淘汰，防结果丢失） |

`POST /config/reload` 热重载：读取时机为**创建 job 时**；已运行 job 不受后续变更影响。

---

## 7. 错误码词汇

- 轮询响应 `error_code`：复用 `tool_result.error_code` 表（`docs/SSE协议.md` §「tool_result.error_code」），新增取值 **`internal`**（worker panic/异常，`retryable=false`）；`timeout` / `cancelled` 语义与现有启发式一致。
- HTTP `ApiError.code`（§3）：`JOB_NOT_FOUND` / `JOB_EXPIRED` / `JOB_OWNERSHIP_MISMATCH` 为**新增**；写入 `crates/crabmate-api-contract/src/error_codes.rs` 与 `docs/命令行契约.md`。

---

## 8. 兼容窗口与双端对齐清单

- **不 bump `SSE_PROTOCOL_VERSION`**：新增均为软字段（`tool_result.tool_job_*`）、新端点、默认 false 参数、默认关配置。旧客户端：忽略软字段；不认识 `async` 的模型不会发起。
- 旧服务端 + 新客户端：`run_command` 的 `async` 参数被 `deny_unknown_fields` 拒绝——**不在**兼容窗口，文档明示。
- 改动控制面分发（`tool_job_finished`）时按 `.cursor/rules/api-sse-chat-protocol.mdc` 执行：
  - [ ] `src/cm_sse_protocol/control_classify.rs`（若动分类）
  - [ ] 金样：本仓 `fixtures/sse_ag_ui_golden.jsonl` + `cargo test --lib golden_ag_ui_classify_matches_expected`
  - [ ] Client：`parser_v2.rs` / `sse_dispatch/types.rs` + `cargo test golden_ag_ui_v2_parser_matches_expected`
  - [ ] `docs/SSE协议.md` 控制面变体一览与 `tool_result` 字段表
- HTTP 新端点：`docs/命令行契约.md` / OpenAPI 同步；错误码表同步。
- 工具契约：`docs/工具说明.md`（`run_command` 的 `async`）、工具 JSON Schema（`tool_parameters_schema_value::<RunCommandArgs>` 自动含新字段）。

---

## 9. 实现落点映射

| 面 | 后端 | Client |
|----|------|--------|
| 参数/启动帧 | `RunCommandArgs` + `tool_params/exec_package.rs`、`runner_run_command` / `execute_run_command.inc.rs` | 参数表单 |
| job 注册表 + worker | 新模块（建议 `src/cm_internal/tool_jobs/`，复用 `subprocess_session`） | — |
| HTTP 端点 | `src/web/routes/`（Bearer 中间件 + 归属校验） | 轮询逻辑（退避） |
| SSE `tool_job_finished` | `cm_sse_protocol/sse/protocol.rs` + 分发 | `parser_v2.rs` |
| 配置 | `cm_config` + `config/tools.toml` | — |
| 观测 | `session_stats_snapshot` 扩展 + job 计数/日志（`tool_job_id`、来源 `job_id`、`duration_ms`） | — |

测试：生命周期、超时/取消（含完成竞态）、过期/410、认证/归属越权、并发与排队上限、worker panic 兜底、`deny_unknown_fields` 回归、金样。
