**语言 / Languages:** 中文（本页）· [English](en/SSE_PROTOCOL.md)

# Agent SSE 控制面协议（`/chat/stream`）

本文档描述 **CrabMate 服务端经 SSE `data:` 行下发的控制面 JSON**，与模型正文的 **纯文本 delta** 区分。**控制面载荷形状**与行分类的单一事实来源为 **`src/cm_sse_protocol`**（`sse/protocol.rs`、`sse/line.rs`）。Client 钉 **`crabmate::cm_sse_protocol`**（`features = ["protocol"]`）；`server` 组合面另有 `sse` 别名，**不要**在 Client 使用。**协议版本号**为 **`SSE_PROTOCOL_VERSION`**（与 Leptos 前端共用）。浏览器消费逻辑在 Client [`frontend/src/api/chat_stream/parser_v2.rs`](https://github.com/noisystreet/crabmate-client/blob/main/frontend/src/api/chat_stream/parser_v2.rs)（AG-UI；回调形状见 [`frontend/src/sse_dispatch/types.rs`](https://github.com/noisystreet/crabmate-client/blob/main/frontend/src/sse_dispatch/types.rs)）。

## 协议版本 `v` 与协商

- 每条控制面 JSON 为对象，**推荐**包含顶层字段 **`v`**（`u8`）。当前版本为 **`2`**，与 **`crabmate::cm_sse_protocol::SSE_PROTOCOL_VERSION`** 一致。
- **缺省**：历史载荷可省略 `v`，反序列化时按 **`SSE_PROTOCOL_VERSION`** 处理（见 `SseMessage` 的 `#[serde(default = "default_sse_v")]`）。
- **请求体（可选）**：`POST /chat` 与 **`POST /chat/stream`** 的 JSON 可带 **`client_sse_protocol`**（`u8`）。**省略**时服务端不据此拒绝（兼容旧客户端）。若 **`client_sse_protocol >` 服务端 `SSE_PROTOCOL_VERSION`** → **HTTP 400**，`ApiError.code` 为 **`SSE_CLIENT_TOO_NEW`**；若为 **`0`** → **`INVALID_SSE_CLIENT_PROTOCOL`**；若为 **正整数且低于**服务端版本 → **`SSE_PROTOCOL_MISMATCH`**。
- **首帧能力**：新流建立后，服务端尽快下发 **`sse_capabilities`**，其中 **`supported_sse_v`** 等于服务端 **`SSE_PROTOCOL_VERSION`**。官方 Leptos 前端在收到该帧时比对本地常量：若 **`supported_sse_v ≠ SSE_PROTOCOL_VERSION`**，触发 `onError` 并停止读流，文案中含 **`SSE_SERVER_TOO_NEW`**（服务端更**新**、前端更**旧**）或 **`SSE_SERVER_TOO_OLD`**（服务端更**旧**、前端更**新**；通常此前已被 **`SSE_CLIENT_TOO_NEW`** 拒绝，保留用于重连重放等边界）。
- **演进**：递增 `v` 时须同步：**`src/cm_sse_protocol`**、本文档与中英 **`docs/en/SSE_PROTOCOL.md`**、**`cargo test --lib --no-default-features --features protocol cm_sse_protocol`**（文档内版本标记自检）。
- **Semver / 发版 / 外仓钉版本**：线协议 `SSE_PROTOCOL_VERSION` 与 Cargo crate semver 是两套轴；破坏性变更、软字段、**当前无 N−1 线协议解码窗口**、git 标签 `client-contract-vX.Y.Z` 钉法见 **[`docs/design/client_contract_versioning.md`](design/client_contract_versioning.md)**。本地/CI 门禁：`bash scripts/check-client-contract.sh`。

## 传输与分帧

- 路由：**`POST /chat/stream`**；响应为 **`text/event-stream`**。（运维向 **`POST /config/reload`** 为 JSON、非 SSE，见 **`docs/配置说明.md`**「配置热重载」。）
- **事件序号 `id:`**：服务端为每个逻辑事件块设置 **`id:`**（单调递增 `u64`，与进程内 `SseStreamHub` 一致）。断线重连时客户端可带请求头 **`Last-Event-ID`**，并在 JSON 体使用 **`stream_resume`**：`{ "job_id": <u64>, "after_seq": <u64> }`（省略 `after_seq` 视为 0）；服务端取 **`max(Last-Event-ID, after_seq)`** 后从环形缓冲重放，再订阅实时广播。**仅单进程内存**：任务结束或进程重启后重连返回 **HTTP 410**，`code` **`STREAM_JOB_GONE`**。用户在 UI 点「停止」须另发 **`POST /chat/stream/{job_id}/cancel`**（`job_id` 与 **`x-stream-job-id`** 相同）置协作取消；**仅 abort SSE 连接不会停止**模型调用与工具（否则无法 `stream_resume`）。新流响应头另含 **`x-stream-job-id`**（与首帧 `sse_capabilities.caps.job_id` 一致）。
- 事件块：以 **空行 `\n\n`** 分隔；块内可有若干 **`data: `** 行。前端将同一块内多行 `data:` **去掉前缀后按 `\n` 拼接**，并**保留前导空格/换行**后直接进入分发（仅在判断 `[DONE]` 哨兵时做 `trim`），避免把“仅空格增量”吞掉导致单词粘连（见 `sendChatStream` 与 `join_sse_data_lines`）。
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
| `assistant_answer_phase`: `true` | 后续纯文本增量为助手 **终答** `content`（此前为思维链 `reasoning_*`）；无思维链时也会在首段正文前下发 | Web：**handled**；切换 `onDelta` 写入目标（思维链区 / 终答区），不当下文 |
| `turn_segment_start` | 回合段开始；体含 **`segment_id`**、**`kind`**（`commentary` \| `answer`）、可选 **`before_tool_call_id`**（本段展示在该工具调用**之前**；晚到 delta 仍挂此锚点） | Web：**handled**；`onTurnSegmentStart` 更新 canonical turn 投影 |
| `turn_segment_end` | 关闭 `turn_segment_start` 所开段；体含 **`segment_id`** | Web：**handled**；`onTurnSegmentEnd` |
| `turn_tool_phase_end`: `true` | 本批工具执行结束；后续正文增量为 post-tool 终答（与 `assistant_answer_phase` 配合） | Web：**handled**；`onTurnToolPhaseEnd` |
| `clarification_questionnaire` | 澄清问卷：模型调用工具 **`present_clarification_questionnaire`** 且成功后，在 **`tool_result` SSE** 之后补发；体含 **`questionnaire_id`**、**`intro`**、**`questions[]`**（`id` / `label` / 可选 `hint` / `required` / `kind`：`text` \| `choice`） | Web：展示表单；用户提交时下一请求体带 **`clarify_questionnaire_answers`**（见 README / OpenAPI）；TUI：`line` 分类为 **ignore** |
| `thinking_trace` | 调试：运行时**默认开启**下发；**`CM_THINKING_TRACE_ENABLED=0`** 时关闭。**不**从 **`[agent]`** TOML 读入。体须含非空 **`op`**（如 **`reasoning_delta`**、**`answer_phase`**、**`tool_call`**、**`tool_done`**）；可选 **`node_id`** / **`parent_id`** / **`title`**、**`chunk`**（推理片段）、**`context_snapshot`**（工具前后上下文摘要，非全文） | Web：「调试台」侧栏累积展示；TUI：`line` 分类为 **ignore** |
| `workspace_changed`: `true` | 工作区已被工具更新 | `onWorkspaceChanged` |
| `tool_call` | 工具调用摘要（执行前）；体含 **`name`**、**`summary`**（与 `summarize_tool_call` 同源）、可选 **`tool_call_id`**（与本轮 `tool_calls[].id` / `tool_result.tool_call_id` 一致，供 Web 将结果写回正确占位气泡）、可选 **`arguments_preview`**（单行截断，与 `execute_tools` 日志同源）、可选 **`arguments`**（配置 **`sse_tool_call_include_arguments`** / **`CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS`** 为真时：启发式脱敏后更长截断） | `onToolCall`（**`summary`**、**`arguments_preview`**、**`arguments`** 至少一项非空则 **handled**） |
| `parsing_tool_calls` | 模型正在流式输出 tool_calls | `onParsingToolCallsChange` |
| `tool_running` | 工具执行中状态 | `onToolStatusChange` |
| `tool_output_chunk` | 工具执行中的输出片段（如 PTY、宿主 **`run_command`**）；**不**进入模型上下文；体内须含非空 **`tool_call_id`**、非负整数 **`seq`**；可选 **`name`**、**`chunk`**（UTF-8 文本，可多次下发由前端拼接）、**`stream`**（`stdout` / `stderr` / `combined`）；最终以 **`tool_result`** 收束 | Web：**handled**，`onToolOutputChunk` 追加至对应 `tool_call_id` 的工具气泡详情；TUI：控制面镜像展示截断摘要 |
| `tool_result` | 工具结束；含 `output` 等 | `onToolResult` |
| `command_approval_request` | `run_command` / 工作流等需用户审批 | `onCommandApprovalRequest` |
| `chat_ui_separator` | 聊天区分隔线；`true` 短、`false` 长 | `onChatUiSeparator` |
| `conversation_saved` | 本会话已成功落库；`revision`（`u64`）供 `POST /chat/branch` 与冲突检测；可选 **`tiktoken_prompt_tokens`**（`prompt_tokens` + `tiktoken_model`，与 `GET /conversation/messages` 同规则） | Leptos：更新 `revision` 与底栏上下文用量（`conversation_prompt_tokens`） |
| `sse_capabilities` | 首帧能力：`supported_sse_v`、`resume_ring_cap`、`job_id`（与 `x-stream-job-id` 一致）；可选软字段 **`terminal_order`**（当前为 **`saved_before_finished`**，旧客户端忽略） | 官方 Web：与本地 **`SSE_PROTOCOL_VERSION`** 校验；匹配则**吞掉**（不当下文）；不匹配则 **`onError`** 并停止。集成方可据此保存 `job_id` 做重连 |
| `stream_draining` | 非终态收尾：模型/工具已结束、正在落盘；`job_id` | Web：可提前进入 Draining 文案；**不**置 `saw_stream_ended` |
| `stream_ended` | 流结束；`job_id`、`reason`（`completed` / `cancelled` / `conflict` / `fallback` / `no_output` / `gone`）；可选 **`tiktoken_prompt_tokens`**（成功路径通常已先发 `conversation_saved`） | Web：**先独立提取并吞掉**（不依赖其它控制面分支命中）；更新底栏用量并停止自动重连 |
| `timeline_log` | 时间线旁注（如审批结果）；**不**进入模型上下文 | `onTimelineLog` |

**`timeline_log.kind` 常用值**：`final_response`（终答标记）；`orchestration_route`（编排路由决议，Web 不渲染为气泡）；`approval_decision` / `tool_result_summary`（审批与工具摘要旁注）；`context_inject` / `context_trim`（本轮注入/skill 与窗口裁剪压缩摘要，软字段，未知 kind 应忽略）。`context_inject` 的 `detail` 为 JSON：`kinds`、`skills`、`forced`；`context_trim` 的 `detail` 含 `count_hit` / `char_hit` / `n_before` / `n_after` / `compress_hits` / `summarized` / `tail_kept`。回合结束写入 `system.name=crabmate_timeline` 同行 JSON（不进模型）；同步裁剪**不计**这些旁注条数，且同 `kind` 的 `context_inject`/`context_trim` **覆盖**旧行。

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
| `structured_preview` | object? | 可选；**`read_file`** / **`read_dir`** / **`list_tree`**：与输出首行 **`crabmate_tool_output`** JSON **同源**的小型副本（**不含**文件正文）；**写盘工具**（如 **`create_file`** / **`modify_file`** / **`apply_patch`** / **`search_replace`** 等）成功时可为 **`preview`**=`workspace_write_diff`，含 **`files[]`**（**`path`**、**`unified_diff`**、**`truncated`**）及 **`preview_truncated`**：供 Web 展示变更预览（**非**审批闸门）；**`run_command`** / **`cargo_*`** / **`rust_rustc`** / **`http_fetch`** / **`http_request`**：与历史 **`crabmate_tool.structured_payload`**（对应 **`schema`**）同源或与之合并；若首行预览与 **`structured_payload`** 同时存在，则为合并对象（**`tool_output_header`** + **`structured_payload`**） |
| `tool_job_id` | string? | **软字段**（旧客户端忽略；不 bump `result_version`）。仅 **`run_command`** 的 **`async:true`** 发起帧：后台任务 id（`tooljob_` + 32 hex 随机，不可枚举，即能力凭证） |
| `tool_job_poll_url` | string? | 同上，相对路径 `GET /tools/jobs/{tool_job_id}`（轮询端点） |
| `tool_job_status` | string? | 同上，发起时恒为 **`queued`**（尚未运行） |

**后台任务软字段说明**：发起 `async:true` 时 `output` 仅为发起确认文案（**不是**执行结果），本帧**省略** `exit_code` / `stdout` / `stderr` / `error_code`；`tool_job_*` 字段**不注入**模型上下文（仅 Web/TUI 展示）。任务状态与终态输出通过轮询 **`GET /tools/jobs/{id}`** 获取（见 `docs/命令行契约.md`）。**`tool_job_finished` 顶层 SSE 键未实现**（Phase 2 可选；旧客户端可忽略未知顶层键）。

### `tool_output_chunk` 体内字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `tool_call_id` | string | 必填非空；与对应 **`tool_call`** / **`tool_result`** 对齐 |
| `seq` | number | 必填；单调序号（`u64`），便于客户端去乱序 |
| `chunk` | string | 可选；本帧增量文本（缺省按空串处理） |
| `name` | string? | 可选；工具名（如 `terminal_session`） |
| `stream` | string? | 可选；`stdout` / `stderr` / `combined` |

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
| `INTERNAL_ERROR` | `chat_job_queue` | 编排层其它失败（**`error`** 为通用用户文案；**`reason_code`** 为截断后的内部摘要，与 `plan_rewrite_exhausted` 的子码表不同） |
| `STEP_RETRY_EXHAUSTED` | `agent_turn` | 单步子重试耗尽（**`error`** 为通用用户文案；**`reason_code`** 含内部摘要） |
| `REPLAN_EXHAUSTED` | `agent_turn` | 全局重规划耗尽（同上） |
| `TIME_LIMIT_EXHAUSTED` | `agent_turn` | 墙钟超时（同上） |
| `TOKEN_LIMIT_EXHAUSTED` | `agent_turn` | Token 预算耗尽（同上） |
| `LLM_REQUEST_FAILED` | `chat_job_queue`（由 `agent_turn` 映射） | 模型 HTTP/传输失败（**`error`** 为脱敏后的网关说明；**429** 等限流见 **`LLM_RATE_LIMIT`**） |
| `LLM_RATE_LIMIT` | `chat_job_queue`（由 `agent_turn` 映射） | 限流 / 配额类（**HTTP 429** 或文案启发式与 `agent_errors::is_quota_or_rate_limit_llm_message` 一致） |
| `turn_aborted` | `chat_job_queue`（由 `agent_turn` 映射） | 编排早停（如 **SSE 接收端已关闭**仍尝试继续）；**`error`** 为用户可读说明 |
| `STREAM_CANCELLED` | `chat_job_queue` | 流被取消且仍可投递时补发（`POST /chat/stream/{job_id}/cancel` 或内部协作取消） |
| `plan_rewrite_exhausted` | `agent_turn/outer_loop` | 终答规划重写次数用尽 |
| `SSE_ENCODE` | `sse/protocol` | `encode_message` 序列化失败兜底 |

**可选字段 `reason_code`**：与 `error` / `code` 同级的字符串子码，供客户端在**同一 `code`** 下做细粒度分支；**`plan_rewrite_exhausted`** 使用下表中的语义化子码；**`INTERNAL_ERROR`** / **`STEP_RETRY_EXHAUSTED`** 等编排失败时为**截断后的内部摘要**（便于排障，旧客户端可忽略）。

**可选字段 `turn_id`**：与响应头 **`x-stream-job-id`**、首帧 **`sse_capabilities.job_id`** 一致（`u64`）；非 Web 路径或历史帧可省略。

**可选字段 `sub_phase`**：失败时所处的编排子阶段，与 PER 心智模型对齐：`planner` \| `executor` \| `reflect`；旧客户端可忽略。

**可选字段 `request_id`**：与当次 HTTP 响应头 **`x-request-id`** / JSON **`ApiError.request_id`** 同值；Web 流任务经 **`TracingChatTurn`** / 队列信封带回（含回合内 `plan_rewrite_exhausted` 与终态失败帧），便于排障复制；**不** bump `SSE_PROTOCOL_VERSION`（软字段）。AG-UI `RUN_ERROR.error` 中序列化为 **`requestId`**（camelCase）。

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
| `STREAM_JOB_GONE` | 410 | **`stream_resume`** 任务不在 hub（见 `chat_stream_handler`）；**`POST /chat/stream/{job_id}/cancel`** 任务未登记时亦为此码与状态 |
| `SSE_CLIENT_TOO_NEW` | 400 | **`client_sse_protocol`** 高于服务端 **`SSE_PROTOCOL_VERSION`** |
| `INVALID_SSE_CLIENT_PROTOCOL` | 400 | **`client_sse_protocol == 0`** |
| `SSE_PROTOCOL_MISMATCH` | 400 | **`client_sse_protocol`** 为正整数且**低于**服务端版本 |
| `INVALID_AT_FILE_REF` | 400 | 用户消息含非法 **`@…`** 文件引用（与 **`read_file`** 规则一致） |
| `INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS` | 400 | 澄清问卷作答体非法（见 `clarification_questionnaire`） |
| `LLM_RATE_LIMIT` | 429 | **`POST /chat`** 模型限流/配额类（与 SSE 同源码） |
| `LLM_REQUEST_FAILED` | 502 等 | **`POST /chat`** 模型 HTTP/传输失败（与上游状态对齐时可能为其它 5xx） |
| `STEP_RETRY_EXHAUSTED` / `REPLAN_EXHAUSTED` / `TIME_LIMIT_EXHAUSTED` / `TOKEN_LIMIT_EXHAUSTED` | 422 | 编排预算类失败（**`message`** 为通用用户文案；**`reason_code`** 省略） |
| `INTERNAL_ERROR` | 500 | 其它编排失败（**`message`** 为通用用户文案；**`reason_code`** 为截断内部摘要，**仅**此类码在 JSON 中带该字段） |
| `STREAM_CANCELLED` | 499 | 用户/协作取消（非标准状态码，与 SSE 同源码；部分客户端可能按 4xx 处理） |

**客户端仅日志/文案用（非服务端下发的 SSE `code`）**：官方 Leptos 在 **`sse_capabilities`** 与本地版本不一致时，`onError` 字符串中含 **`SSE_SERVER_TOO_NEW`** 或 **`SSE_SERVER_TOO_OLD`**。

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
| `repeated_tool_failure_short_circuit` | 同一工具调用签名已失败，本次被编排层短路 |
| `repeated_tool_family_failure_short_circuit` | 同一失败族已发生，本次同类工具调用被编排层短路 |
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

队列满、鉴权失败等可能返回 **HTTP 4xx/5xx + JSON**（如 `code: "QUEUE_FULL"`），**不**经 SSE `data:`。完整 **`ApiError.code`** 表维护在 **`docs/命令行契约.md`**（HTTP 契约）；**本文件**以 SSE 控制面与 **`client_sse_protocol`** 相关 HTTP 码为主，并与上文流错误表互补。

## 双端对齐检查清单

变更以下任一时，须同步另一方及本文档：

1. **`src/cm_sse_protocol`**：`SSE_PROTOCOL_VERSION`；`sse/protocol.rs`：`SsePayload`、`SseErrorBody`、`ToolResultBody`（生产默认 **`V2Encoder`** / `default_encoder()`）
2. **`src/cm_sse_protocol`**：`sse_frame.rs`（`parse_sse_event_id` / `join_sse_data_lines` / `is_sse_done_sentinel` / `extract_stream_ended_reason`）与 `control_extract.rs`（`extract_*` 家族）在前端消费语义变更时同步
3. Client `frontend/src/api/chat_stream/parser_v2.rs` 与 **`frontend/src/api/`**（**`chat_stream/`** 等）：控制面分类与分发分支顺序、请求体中的 **`client_sse_protocol`**
4. `src/cm_sse_protocol/sse/line.rs`：`classify_agent_sse_line`（与前端分支语义一致；可选/未来 TUI）
5. 新增 `encode_message(SsePayload::…)` 的调用点

## 契约测试（控制面分类）

现行 Web（AG-UI）由 Client **`frontend/src/api/chat_stream/parser_v2.rs`** 判定 `handled` / `plain` / `stream_ended`，金样 **`fixtures/sse_ag_ui_golden.jsonl`**。V1 形状的 `stop` / `handled` / `plain` 分类函数 **`classify_sse_control_outcome`**（`control_classify.rs`）仍供非 Web 消费者使用，参考向量见 **`fixtures/sse_control_golden.jsonl`**。

---

## 附录：AG-UI 协议（v2，开发中）

CrabMate 正逐步从自定义 SSE 控制面协议（v1，本文档主体）迁移至 [AG-UI 协议](https://docs.ag-ui.com/concepts/events)（v2）。迁移期间两种格式共存。

### 格式与信封

AG-UI 事件为单行 JSON，无 `v` 字段或 `SseMessage` 信封：

```json
{"type":"RUN_FINISHED","threadId":"th-1","runId":"run-1"}
```

通过 `"type"` 字段区分事件类型（`SCREAMING_SNAKE_CASE`）。

### 生命周期

| 事件 | 含义 |
|------|------|
| `RUN_STARTED` | 回合开始（首帧） |
| `RUN_FINISHED` | 回合正常结束 → 前端进入收尾并最终 `on_done` + `saw_stream_ended`（见下「终态顺序」）。可选 CrabMate 扩展字段 **`tiktokenPromptTokens`**（与 v1 `stream_ended.tiktoken_prompt_tokens` 同源；成功路径通常已先发 `conversation_saved`，本字段作后备） |
| `RUN_ERROR` | 回合出错 → 前端 `on_error` + `saw_stream_ended` |

**CUSTOM `stream_draining`**：模型/工具执行已结束、正在落盘等收尾；**非**终态。官方 Web 可提前进入 Draining 文案，**不**置 `saw_stream_ended`；仍须读完 body。

**终态顺序（Phase E1，服务端现行）**

- **服务端（新序）**：可选 **`stream_draining`** → 落盘 → **`conversation_saved`** →（可选 `STATE_SNAPSHOT`）→ **最后** `RUN_FINISHED` / `RUN_ERROR`。首帧 `sse_capabilities.terminal_order = saved_before_finished`（软字段，**不** bump `SSE_PROTOCOL_VERSION`）。
- **官方 Web（双序）**：仍在 `RUN_FINISHED` 后继续读 body；亦接受旧序（`RUN_FINISHED` 后再来 `conversation_saved`）。`on_done` 至多一次，由 body 消费完成驱动（见 Client `frontend/src/api/chat_stream/sse_frame.rs`）。
- **后续收缩（E4）**：待删除「终态后业务帧」兼容等；见 **`docs/Turn布局设计.md` §16**。

### 工具调用

| 事件 | 含义 |
|------|------|
| `TOOL_CALL_START` | 工具声明（名称 + id） |
| `TOOL_CALL_ARGS` | 工具参数 |
| `TOOL_CALL_END` | 工具声明结束 |
| `TOOL_CALL_RESULT` | 工具执行结果（`metadata.partial` 为 `true` 时表示输出片段） |

### 文本消息（预留）

`TEXT_MESSAGE_START` / `CONTENT` / `END`、`REASONING_MESSAGE_START` / `CONTENT` / `END` 为 AG-UI 标准事件，前端已注册解析但**当前后端不发送**。

### CUSTOM 扩展事件

CrabMate 专有事件通过 `{"type":"CUSTOM","customType":"…","data":{…}}` 承载：

| `customType` | 对应 v1 事件 | 前端回调 |
|-------------|-------------|---------|
| `tool_running` | `tool_running` | `on_tool_status` |
| `parsing_tool_calls` | `parsing_tool_calls` | `on_parsing_tool_calls` |
| `assistant_answer_phase` | `assistant_answer_phase` | `on_assistant_answer_phase` |
| `turn_segment_start/end` | `turn_segment_start/end` | `on_turn_segment_start/end` |
| `turn_tool_phase_end` | `turn_tool_phase_end` | `on_turn_tool_phase_end` |
| `workspace_changed` | `workspace_changed` | `on_workspace_changed` |
| `command_approval` | `command_approval_request` | `on_approval` |
| `clarification_questionnaire` | `clarification_questionnaire` | `on_clarification_questionnaire` |
| `thinking_trace` | `thinking_trace` | `on_thinking_trace` |
| `timeline_log` | `timeline_log` | `on_timeline_log` |
| `conversation_saved` | `conversation_saved` | `on_conversation_revision`（`data.revision`；可选 **`data.tiktokenPromptTokens`** → 底栏上下文用量） |
| `stream_draining` | `stream_draining` | `on_stream_draining` → Draining 文案（不清 abort/resume；不置 `saw_stream_ended`） |
| `chat_ui_separator` | `chat_ui_separator` | 忽略 |
| `sse_capabilities` | `sse_capabilities` | 忽略 |

### 状态同步

| 事件 | 含义 |
|------|------|
| `STATE_SNAPSHOT` | 工具批结束等边界发送的完整回合状态快照（当前为最小标记，待扩展） |
| `STATE_DELTA` | 状态增量（预留） |

### 切换机制

- `POST /chat/stream` 请求体可选字段 `client_sse_protocol`：当前生产默认 **v2（AG-UI）**；服务端编码走 **`V2Encoder`**
- Web 前端由 **`parser_v2.rs`** 消费；V1 形状分类（`classify_sse_control_outcome`）仍供非 Web 消费者使用
- 版本常量：`SSE_PROTOCOL_VERSION=2`

### 金样测试

AG-UI 事件的 V2Parser 分类验证见 `fixtures/sse_ag_ui_golden.jsonl`，由 Client `frontend/src/api/chat_stream/parser_v2.rs` 的 `golden_ag_ui_v2_parser_matches_expected` 测试驱动。

- **`fixtures/sse_control_golden.jsonl`**：V1 JSON 形状参考向量（每行 `描述<TAB>JSON<TAB>期望分类`；`#` 开头行为注释），与 **`classify_sse_control_outcome`** 对齐维护。
- **Web AG-UI**：`cd ../crabmate-client/frontend && cargo test golden_ag_ui_v2_parser_matches_expected`。
- **`fixtures/http_sse_failure_path_golden.jsonl`**：HTTP/SSE **失败路径**契约（`RunAgentTurnError` → `code` / HTTP 状态 / `ApiError`↔SSE `reason_code` 分流；`client_sse_protocol` 握手；`QUEUE_FULL` / `STREAM_JOB_GONE` 等码常量）。本仓：`golden_http_sse_failure_path_*`（亦由 **`./scripts/check-sse-protocol.sh`** 跑）。
若新增控制面顶层键且 Web 应消费：在 **`parser_v2.rs`** 增加分支后，同步 **`fixtures/sse_ag_ui_golden.jsonl`**；若 IM/V1 仍需识别，再改 **`control_classify.rs`** 与 **`sse_control_golden.jsonl`**。

## 契约测试（`crabmate_tool` 历史信封）

- **`src/cm_tools/fixtures/tool_result_envelope_golden.jsonl`**：每行 `描述<TAB>单行 JSON`（`#` 行为注释）；与 **`tool_result::normalize_tool_message_content`** + **`NormalizedToolEnvelope::encode_to_message_line`** round-trip 对齐。
- **Rust**：`cargo test tool_result_envelope_golden`。

---

维护者备注：表格与枚举力求与代码一致；若发现漂移，以 **`protocol.rs` + Client `frontend/src/api/chat_stream/parser_v2.rs`** 为准并修正本文档。
