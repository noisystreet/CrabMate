**语言 / Languages:** 中文（本页）· [English](en/SSE_PROTOCOL.md)

# Agent SSE 控制面协议（`/chat/stream`）

本文档描述 **CrabMate 服务端经 SSE `data:` 行下发的控制面 JSON**，与模型正文的 **纯文本 delta** 区分。**控制面载荷形状**的单一事实来源为 Rust `src/sse/protocol.rs`；**协议版本号**与 Leptos 前端共用 workspace crate **`crabmate-sse-protocol`**（常量 **`SSE_PROTOCOL_VERSION`**，`protocol.rs` 再导出同名常量）。浏览器消费逻辑在 `frontend-leptos/src/sse_dispatch.rs`（由 `frontend-leptos/src/api.rs` 调用）。Rust 侧行分类见 `src/sse/line.rs`（`classify_agent_sse_line`），须与本表语义一致。

## 协议版本 `v` 与协商

- 每条控制面 JSON 为对象，**推荐**包含顶层字段 **`v`**（`u8`）。当前版本为 **`1`**，与 **`crabmate_sse_protocol::SSE_PROTOCOL_VERSION`**（及 `sse::protocol::SSE_PROTOCOL_VERSION`）一致。
- **缺省**：历史载荷可省略 `v`，反序列化时按 **`SSE_PROTOCOL_VERSION`** 处理（见 `SseMessage` 的 `#[serde(default = "default_sse_v")]`）。
- **请求体（可选）**：`POST /chat` 与 **`POST /chat/stream`** 的 JSON 可带 **`client_sse_protocol`**（`u8`）。**省略**时服务端不据此拒绝（兼容旧客户端）。若 **`client_sse_protocol >` 服务端 `SSE_PROTOCOL_VERSION`** → **HTTP 400**，`ApiError.code` 为 **`SSE_CLIENT_TOO_NEW`**；若为 **`0`** → **`INVALID_SSE_CLIENT_PROTOCOL`**。
- **首帧能力**：新流建立后，服务端尽快下发 **`sse_capabilities`**，其中 **`supported_sse_v`** 等于服务端 **`SSE_PROTOCOL_VERSION`**。官方 Leptos 前端在收到该帧时比对本地常量：若 **`supported_sse_v ≠ SSE_PROTOCOL_VERSION`**，触发 `onError` 并停止读流，文案中含 **`SSE_SERVER_TOO_NEW`**（服务端更**新**、前端更**旧**）或 **`SSE_SERVER_TOO_OLD`**（服务端更**旧**、前端更**新**；通常此前已被 **`SSE_CLIENT_TOO_NEW`** 拒绝，保留用于重连重放等边界）。
- **演进**：递增 `v` 时须同步：**`crates/crabmate-sse-protocol`**、本文档与中英 **`docs/en/SSE_PROTOCOL.md`**、**`cargo test -p crabmate-sse-protocol`**（文档内版本标记自检）。

## 传输与分帧

- 路由：**`POST /chat/stream`**；响应为 **`text/event-stream`**。（运维向 **`POST /config/reload`** 为 JSON、非 SSE，见 **`docs/CONFIGURATION.md`**「配置热重载」。）
- **事件序号 `id:`**：服务端为每个逻辑事件块设置 **`id:`**（单调递增 `u64`，与进程内 `SseStreamHub` 一致）。断线重连时客户端可带请求头 **`Last-Event-ID`**，并在 JSON 体使用 **`stream_resume`**：`{ "job_id": <u64>, "after_seq": <u64> }`（省略 `after_seq` 视为 0）；服务端取 **`max(Last-Event-ID, after_seq)`** 后从环形缓冲重放，再订阅实时广播。**仅单进程内存**：任务结束或进程重启后重连返回 **HTTP 410**，`code` **`STREAM_JOB_GONE`**。新流响应头另含 **`x-stream-job-id`**（与首帧 `sse_capabilities.caps.job_id` 一致）。
- 事件块：以 **空行 `\n\n`** 分隔；块内可有若干 **`data: `** 行。前端将同一块内多行 `data:` **去掉前缀后按 `\n` 拼接**，再 `trim()` 得到一条待解析字符串（见 `sendChatStream`）。
- **正文 delta**：拼接后的字符串若 **不是** 控制面 JSON（解析失败），或解析后判定为 **`plain`**，则作为助手正文片段交给 `onDelta`。
- **流结束**：可能收到字面量 **`[DONE]`**（与 OpenAI 兼容习惯一致），前端忽略，不当作正文。另见控制面 **`stream_ended`**。

## 信封形状

控制面载荷序列化为**单行 JSON**，逻辑结构为：

```json
{ "v": 1, …payload… }
```

`SsePayload` 使用 **`serde(untagged)`**，故 JSON 上**不会出现** `"SsePayload"` 包装键；由字段形状区分变体（与 `api.ts` 的 `SseControlPayload` 一致）。

## 与模型正文的区分（`error` 陷阱）

- 若 JSON 仅有 **`error`** 字符串、且 **`code` 缺失或为空**，则 **不得**视为协议错误：模型思维链里可能出现形如 `{"error":"…"}` 的示例对象。
- **协议流错误**（应停止流、`onError`）：必须同时带 **非空 `code`**（`tryDispatchSseControlPayload` / `classify_agent_sse_line` 均按此规则）。
- 服务端经 **`encode_message`** 下发的 `SsePayload::Error` **应始终**带非空 `code`；序列化失败时的兜底为 `code: "SSE_ENCODE"`。

## 控制面变体一览

下列为**顶层键**（与 `v` 并列）。同一对象只应命中一行；解析顺序以前端 `tryDispatchSseControlPayload` 为准。

| 顶层键 / 形状 | 含义 | 前端处理 |
|---------------|------|----------|
| `error` + **`code`** | 流级失败 | `onError`，**停止**读取 |
| `plan_required` | 预留（如须补充结构化规划） | `onPlanRequired`，继续 |
| `staged_plan_started` | 分阶段规划开始 | `onStagedPlanStarted` |
| `staged_plan_step_started` | 单步开始；负载含 `plan_id`、`step_id`、`step_index`、`total_steps`、`description`，可选 `executor_kind`（`review_readonly` / `patch_write` / `test_runner`，与规划 JSON 一致） | `onStagedPlanStepStarted` |
| `staged_plan_step_finished` | 单步结束；`status`: `ok` / `cancelled` / `failed`；可选 `executor_kind`（与 `staged_plan_step_started` 对齐） | `onStagedPlanStepFinished` |
| `staged_plan_finished` | 整轮计划结束；`status` 同上 | `onStagedPlanFinished` |
| `clarification_questionnaire` | 澄清问卷：模型调用工具 **`present_clarification_questionnaire`** 且成功后，在 **`tool_result` SSE** 之后补发；体含 **`questionnaire_id`**、**`intro`**、**`questions[]`**（`id` / `label` / 可选 `hint` / `required` / `kind`：`text` \| `choice`） | Web：展示表单；用户提交时下一请求体带 **`clarify_questionnaire_answers`**（见 README / OpenAPI）；TUI：`line` 分类为 **ignore** |
| `workspace_changed`: `true` | 工作区已被工具更新 | `onWorkspaceChanged` |
| `tool_call` | 工具调用摘要（执行前）；体含 **`name`**、**`summary`**（与 `summarize_tool_call` 同源）、可选 **`arguments_preview`**（单行截断，与 `execute_tools` 日志同源）、可选 **`arguments`**（配置 **`sse_tool_call_include_arguments`** / **`AGENT_SSE_TOOL_CALL_INCLUDE_ARGUMENTS`** 为真时：启发式脱敏后更长截断） | `onToolCall`（**`summary`**、**`arguments_preview`**、**`arguments`** 至少一项非空则 **handled**） |
| `parsing_tool_calls` | 模型正在流式输出 tool_calls | `onParsingToolCallsChange` |
| `tool_running` | 工具执行中状态 | `onToolStatusChange` |
| `tool_result` | 工具结束；含 `output` 等 | `onToolResult` |
| `command_approval_request` | `run_command` / 工作流等需用户审批 | `onCommandApprovalRequest` |
| `staged_plan_notice` / `staged_plan_notice_clear` | 规划进度文本（TUI 等）；Web **吞掉**不当下文 | `handled`，不 `onDelta` |
| `chat_ui_separator` | 聊天区分隔线；`true` 短、`false` 长 | `onChatUiSeparator` |
| `conversation_saved` | 本会话已成功落库；`revision`（`u64`）供 `POST /chat/branch` 与冲突检测 | Leptos：`sse_dispatch` 解析后更新内存中的 `revision`；`onConversationSaved` |
| `sse_capabilities` | 首帧能力：`supported_sse_v`、`resume_ring_cap`、`job_id`（与 `x-stream-job-id` 一致） | 官方 Web：与本地 **`SSE_PROTOCOL_VERSION`** 校验；匹配则**吞掉**（不当下文）；不匹配则 **`onError`** 并停止。集成方可据此保存 `job_id` 做重连 |
| `stream_ended` | 流结束；`job_id`、`reason`（`completed` / `cancelled`） | Web：**吞掉**；客户端可据此停止自动重连 |
| `timeline_log` | 时间线旁注（如审批结果）；**不**进入模型上下文 | `onTimelineLog` |

### `tool_result` 常用字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | 工具名 |
| `result_version` | number | **工具结果载荷版本**，与写入历史的 **`crabmate_tool.v`** 对齐（当前 **1**）。**区别于**整条控制面顶层的 **`v`**（`SSE_PROTOCOL_VERSION`）。缺省反序列化为 **1**。 |
| `summary` | string? | 与 `summarize_tool_call` 同源 |
| `output` | string | 完整文本输出（前端展示依赖场景） |
| `ok` | bool? | 是否成功 |
| `exit_code` | number? | 如命令工具 |
| `error_code` | string? | 机器可读，见下表 |
| `failure_category` | string? | 粗粒度失败分类，与 Rust **`tool_result::ToolFailureCategory::as_str`** 及历史 **`crabmate_tool.failure_category`** 同源；由 **`error_code`** 推导（成功帧省略）。稳定取值见下文 **`failure_category` 枚举** |
| `stdout` / `stderr` | string? | 分流输出（若有） |
| `retryable` | bool? | 失败时可选；与 `crabmate_tool.retryable` 一致，**启发式**（如超时、工作流汇合类），**非**执行保证 |
| `tool_call_id` | string? | 与 OpenAI 兼容的本次 `tool_calls[].id`，便于与助手消息对齐 |
| `execution_mode` | string? | `serial`（串行或含写/审批路径）或 `parallel_readonly_batch`（同轮只读并行批） |
| `parallel_batch_id` | string? | 仅 `parallel_readonly_batch`；同批内多工具共享（形如 `prb-<n>`） |

### `command_approval_request`

| 字段 | 说明 |
|------|------|
| `command` | 命令名 |
| `args` | 参数串 |
| `allowlist_key` | 可选；永久允许时写入白名单的键 |

## 流错误 `code` 枚举（`error` + `code`）

以下为 **当前代码路径**会经 SSE `data:` 下发的 **`SsePayload::Error`**（`error` + 非空 `code`）。与「仅有 `error` 字符串、无 `code`」的模型正文片段区分见上文「与模型正文的区分」。

| `code` | 来源（模块） | 含义 |
|--------|----------------|------|
| `CONVERSATION_CONFLICT` | `web/chat_handlers/conflict`、`chat_job_queue`（流式保存冲突） | 会话 revision / 保存冲突 |
| `INTERNAL_ERROR` | `chat_job_queue` | `run_agent_turn` 失败等非取消类错误（用户可见兜底文案） |
| `STREAM_CANCELLED` | `chat_job_queue` | 流被取消且仍可投递时补发（与协作取消配合） |
| `plan_rewrite_exhausted` | `agent_turn/outer_loop`、`agent_turn/staged` | 终答规划重写次数用尽 |
| `SSE_ENCODE` | `sse/protocol` | `encode_message` 序列化失败兜底 |

**可选字段 `reason_code`**：与 `error` / `code` 同级的字符串子码，供客户端在**同一 `code`** 下做细粒度分支（当前主要用于 `plan_rewrite_exhausted`）；旧实现可忽略。

#### `plan_rewrite_exhausted` 的 `reason_code`

表示用尽重写次数时**最后一轮**终答仍不满足规划规则的大致类别。

| `reason_code` | 含义 |
|----------------|------|
| `plan_missing` | 正文无可解析的 `agent_reply_plan` v1 |
| `plan_layer_count_mismatch` | `steps` 条数低于 `workflow_validate` 的 `layer_count` 要求 |
| `plan_workflow_node_ids_invalid` | `workflow_node_id` 与最近工作流节点 id 集合不一致 |
| `plan_workflow_node_coverage_incomplete` | 严格模式下未覆盖全部工作流节点 id |
| `plan_validate_only_node_binding_mismatch` | `workflow_validate_only` 后规划未与 `nodes[].id` 一一绑定（步数、逐步 `workflow_node_id` 或多重集合不一致） |
| `plan_semantic_inconsistent` | 侧向语义校验判定与最近工具结果矛盾 |
| `plan_rewrite_exhausted_other` | 防御性兜底（主路径不应出现） |

**仅 HTTP、不经 SSE `data:`**（`POST /chat`、`POST /chat/stream` 的 JSON 体，`ApiError`）与流式相关的补充码：

| `code` | HTTP | 说明 |
|--------|------|------|
| `STREAM_JOB_GONE` | 410 | **`stream_resume`** 任务不在 hub（见 `chat_stream_handler`） |
| `SSE_CLIENT_TOO_NEW` | 400 | 请求体 **`client_sse_protocol`** 大于服务端 **`SSE_PROTOCOL_VERSION`** |
| `INVALID_SSE_CLIENT_PROTOCOL` | 400 | **`client_sse_protocol == 0`** |
| `INVALID_AT_FILE_REF` | 400 | 用户消息含非法 **`@…`** 文件引用（如绝对路径或 **`/`** 开头的「伪相对」）；须为相对当前工作区根的相对路径，语义与 **`read_file`** 一致 |
| `INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS` | 400 | **`clarify_questionnaire_answers`** 形状非法（`questionnaire_id` / `answers` 键值与长度限制）；见 `clarification_questionnaire` 模块 |

**客户端仅日志/文案用（非服务端下发的 SSE `code`）**：官方 Leptos 在 **`sse_capabilities`** 与本地版本不一致时，`onError` 字符串中含 **`SSE_SERVER_TOO_NEW`** 或 **`SSE_SERVER_TOO_OLD`**。

**历史/文档保留（当前实现通常不再下发对应 SSE 帧）**：`staged_plan_tool_calls`、`staged_plan_invalid`（旧版规划轮行为；见 `chat_job_queue` 对 `staged_plan_invalid:` 前缀错误的日志分支，一般不序列化为控制面错误）。

## `tool_result.error_code`（工具 / 工作流）

工具失败时 **`tool_result.error_code`** 为机器可读分类（与流错误 `code` 不同通道）。常见值：

| `error_code` | 典型场景 |
|--------------|-----------|
| `invalid_args` | 参数解析错误（`tool_result` 解析启发式） |
| `command_not_allowed` | 命令不在白名单 |
| `command_denied` | 用户/策略拒绝命令 |
| `workspace_not_set` | 未设置工作区 |
| `timeout` | 执行超时 |
| `unknown_tool` | 未知工具名 |
| `approval_required` | 待审批 |
| `approval_denied` | 审批拒绝 |
| `workflow_semaphore_closed` | 工作流并发关闭 |
| `workflow_node_missing_result` | 工作流节点缺结果 |
| `workflow_tool_join_error` | 工作流工具任务 join 失败 |
| `{tool_name}_failed` | 通用：某工具失败（如 `run_command_failed`） |

完整启发式见 `src/tool_result/mod.rs`（`classify_error_code`）；**`error_code` → `failure_category`** 映射见 **`src/tool_result/tool_error.rs`**（**`failure_category_for_error_code`**，与 **`ToolFailureCategory`** 一致）。工作流专用见 `src/agent/workflow/execute.rs`。

### `tool_result.failure_category`（与 `crabmate_tool.failure_category`）

与 Rust 枚举 **`tool_result::ToolFailureCategory`** 的 **`as_str()`** 一致，便于客户端 **`match`** 而不过度依赖自由字符串 **`error_code`**：

| `failure_category` | 含义 |
|--------------------|------|
| `invalid_input` | 参数 / JSON / 必填字段等 |
| `policy_denied` | 白名单、限流、策略拒绝等 |
| `workspace` | 工作区未设置、路径不在允许根内等 |
| `timeout` | 工具或子进程超时 |
| `external` | 外部命令非零退出、IO、HTTP 业务失败等 |
| `internal` | 工具内部不变量（少见） |
| `unknown` | 无法归类或未知工具 |

**说明**：新出现的 **`error_code`** 可能暂时落入 **`unknown`** 或经 `_failed` 后缀规则归入 **`external`**；细化映射时在 **`failure_category_for_error_code`** 中扩展。

## 与 `POST /chat` HTTP 错误的区别

队列满、鉴权失败等可能返回 **HTTP 4xx/5xx + JSON**（如 `code: "QUEUE_FULL"`），**不**经 SSE `data:`。完整 **`ApiError.code`** 表维护在 **`docs/CLI_CONTRACT.md`**（HTTP 契约）；**本文件**以 SSE 控制面与 **`client_sse_protocol`** 相关 HTTP 码为主，并与上文流错误表互补。

## 双端对齐检查清单

变更以下任一时，须同步另一方及本文档：

1. **`crates/crabmate-sse-protocol`**：`SSE_PROTOCOL_VERSION`；`src/sse/protocol.rs`：`SsePayload`、`SseErrorBody`、`ToolResultBody`（版本常量由 crate 提供并在 `protocol` 再导出）
2. `frontend-leptos/src/sse_dispatch.rs` 与 `frontend-leptos/src/api.rs`：控制面分类与分发分支顺序、请求体中的 **`client_sse_protocol`**
3. `src/sse/line.rs`：`classify_agent_sse_line`（与前端分支语义一致）
4. 新增 `encode_message(SsePayload::…)` 的调用点

## 契约测试（控制面分类）

Web 将一条合并后的 `data:` 字符串解析为 JSON 后，按**固定顺序**判定 `stop` / `handled` / `plain`（见 `frontend-leptos/src/sse_dispatch.rs`）。Rust 侧镜像实现为 **`src/sse/control_dispatch_mirror.rs`**（仅 `cfg(test)`），与 **同一份金样**对齐：

- **`fixtures/sse_control_golden.jsonl`**：每行 `描述<TAB>JSON<TAB>期望分类`（`#` 开头行为注释）。
- **Rust**：`cargo test golden_sse_control`（或 `cargo test control_dispatch_mirror`）。
若新增控制面顶层键且 Web 应消费：在 `frontend-leptos/src/sse_dispatch.rs` 增加分支后，同步 **`control_dispatch_mirror::classify_sse_control_outcome`** 与金样行。

## 契约测试（`crabmate_tool` 历史信封）

- **`fixtures/tool_result_envelope_golden.jsonl`**：每行 `描述<TAB>单行 JSON`（`#` 行为注释）；与 **`tool_result::normalize_tool_message_content`** + **`NormalizedToolEnvelope::encode_to_message_line`** round-trip 对齐。
- **Rust**：`cargo test tool_result_envelope_golden`。

---

维护者备注：表格与枚举力求与代码一致；若发现漂移，以 **`protocol.rs` + `frontend-leptos/src/sse_dispatch.rs`** 为准并修正本文档。
