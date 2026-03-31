**语言 / Languages:** 中文（本页）· [English](en/SSE_PROTOCOL.md)

# Agent SSE 控制面协议（`/chat/stream`）

本文档描述 **CrabMate 服务端经 SSE `data:` 行下发的控制面 JSON**，与模型正文的 **纯文本 delta** 区分。实现与类型的**单一事实来源**为 Rust `src/sse/protocol.rs`；浏览器消费逻辑在 `frontend-leptos/src/sse_dispatch.rs`（由 `frontend-leptos/src/api.rs` 调用）。Rust 侧行分类见 `src/sse/line.rs`（`classify_agent_sse_line`），须与本表语义一致。

## 协议版本 `v`

- 每条控制面 JSON 为对象，**推荐**包含顶层字段 **`v`**（`u8`）。当前版本为 **`1`**，与常量 **`sse::protocol::SSE_PROTOCOL_VERSION`** 对齐。
- **缺省**：历史载荷可省略 `v`，反序列化时按 **`1`** 处理（见 `SseMessage` 的 `#[serde(default = "default_sse_v")]`）。
- **演进**：递增 `v` 时须同步更新本文档、`SSE_PROTOCOL_VERSION`（Rust）、`SSE_PROTOCOL_VERSION`（TS），并在前端对未知版本做降级或显式报错（当前实现按字段形状解析，未强制校验 `v`）。

## 传输与分帧

- 路由：**`POST /chat/stream`**；响应为 **`text/event-stream`**。（运维向 **`POST /config/reload`** 为 JSON、非 SSE，见 **`docs/CONFIGURATION.md`**「配置热重载」。）
- 事件块：以 **空行 `\n\n`** 分隔；块内可有若干 **`data: `** 行。前端将同一块内多行 `data:` **去掉前缀后按 `\n` 拼接**，再 `trim()` 得到一条待解析字符串（见 `sendChatStream`）。
- **正文 delta**：拼接后的字符串若 **不是** 控制面 JSON（解析失败），或解析后判定为 **`plain`**，则作为助手正文片段交给 `onDelta`。
- **流结束**：可能收到字面量 **`[DONE]`**（与 OpenAI 兼容习惯一致），前端忽略，不当作正文。

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
| `staged_plan_step_started` | 单步开始 | `onStagedPlanStepStarted` |
| `staged_plan_step_finished` | 单步结束；`status`: `ok` / `cancelled` / `failed` | `onStagedPlanStepFinished` |
| `staged_plan_finished` | 整轮计划结束；`status` 同上 | `onStagedPlanFinished` |
| `workspace_changed`: `true` | 工作区已被工具更新 | `onWorkspaceChanged` |
| `tool_call` | 工具调用摘要（执行前） | `onToolCall`（需 `summary`） |
| `parsing_tool_calls` | 模型正在流式输出 tool_calls | `onParsingToolCallsChange` |
| `tool_running` | 工具执行中状态 | `onToolStatusChange` |
| `tool_result` | 工具结束；含 `output` 等 | `onToolResult` |
| `command_approval_request` | `run_command` / 工作流等需用户审批 | `onCommandApprovalRequest` |
| `staged_plan_notice` / `staged_plan_notice_clear` | 规划进度文本（TUI 等）；Web **吞掉**不当下文 | `handled`，不 `onDelta` |
| `chat_ui_separator` | 聊天区分隔线；`true` 短、`false` 长 | `onChatUiSeparator` |
| `conversation_saved` | 本会话已成功落库；`revision`（`u64`）供 `POST /chat/branch` 与冲突检测 | `onConversationSaved` |
| `timeline_log` | 时间线旁注（如审批结果）；**不**进入模型上下文 | `onTimelineLog` |

### `tool_result` 常用字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | 工具名 |
| `summary` | string? | 与 `summarize_tool_call` 同源 |
| `output` | string | 完整文本输出（前端展示依赖场景） |
| `ok` | bool? | 是否成功 |
| `exit_code` | number? | 如命令工具 |
| `error_code` | string? | 机器可读，见下表 |
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

以下为当前 Rust 路径会下发的 **SSE 流错误**码（非 HTTP JSON）。新增码时须更新本表并改 `api.ts` 若需分支逻辑。

| `code` | 来源（模块） | 含义 |
|--------|----------------|------|
| `CONVERSATION_CONFLICT` | `web/chat_handlers`、`chat_job_queue` | 会话版本冲突 / 保存冲突 |
| `INTERNAL_ERROR` | `chat_job_queue` | 队列或内部未预期错误 |
| `STREAM_CANCELLED` | `chat_job_queue` | 流式任务被取消（如客户端断开导致协作取消，且 SSE 仍可投递时补发）；与 `llm::api::stream_chat` 在 **`out` 发送失败** 时置位的取消标志配合，减少静默空转 |
| `staged_plan_tool_calls` | `agent_turn/staged` | （**保留/兼容**）旧版在规划轮因原生 `tool_calls` 报错；**当前**规划轮丢弃原生 `tool_calls` 并从正文 DSML 物化，**通常不再下发** |
| `staged_plan_invalid` | （保留/兼容） | 旧版在规划 JSON 无效时下发；**当前服务端**对该情况已改为降级为常规循环，**通常不再出现** |
| `plan_rewrite_exhausted` | `agent_turn/outer_loop` | 终答规划重写次数用尽 |
| `SSE_ENCODE` | `sse/protocol` | 控制面 JSON 序列化失败（兜底） |

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

完整启发式见 `src/tool_result.rs`（`classify_error_code`）；工作流专用见 `src/agent/workflow/execute.rs`。

## 与 `POST /chat` HTTP 错误的区别

队列满、鉴权失败等可能返回 **HTTP 4xx/5xx + JSON**（如 `code: "QUEUE_FULL"`），**不**经 SSE `data:`。此类码见 `web/chat_handlers` 与 README 的 API 说明；**本文件仅覆盖 SSE 流内控制面。**

## 双端对齐检查清单

变更以下任一时，须同步另一方及本文档：

1. `src/sse/protocol.rs`：`SsePayload`、`SseErrorBody`、`ToolResultBody`、`SSE_PROTOCOL_VERSION`
2. `frontend-leptos/src/sse_dispatch.rs` 与 `frontend-leptos/src/api.rs`：控制面分类与分发分支顺序
3. `src/sse/line.rs`：`classify_agent_sse_line`（与前端分支语义一致）
4. 新增 `encode_message(SsePayload::…)` 的调用点

## 契约测试（控制面分类）

Web 将一条合并后的 `data:` 字符串解析为 JSON 后，按**固定顺序**判定 `stop` / `handled` / `plain`（见 `frontend-leptos/src/sse_dispatch.rs`）。Rust 侧镜像实现为 **`src/sse/control_dispatch_mirror.rs`**（仅 `cfg(test)`），与 **同一份金样**对齐：

- **`fixtures/sse_control_golden.jsonl`**：每行 `描述<TAB>JSON<TAB>期望分类`（`#` 开头行为注释）。
- **Rust**：`cargo test golden_sse_control`（或 `cargo test control_dispatch_mirror`）。
若新增控制面顶层键且 Web 应消费：在 `frontend-leptos/src/sse_dispatch.rs` 增加分支后，同步 **`control_dispatch_mirror::classify_sse_control_outcome`** 与金样行。

---

维护者备注：表格与枚举力求与代码一致；若发现漂移，以 **`protocol.rs` + `frontend-leptos/src/sse_dispatch.rs`** 为准并修正本文档。
