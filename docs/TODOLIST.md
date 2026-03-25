# 后续修复与完善清单

本文仅保留**未完成**待办。**某项完成后必须从本文件删除该条目**（勿长期保留 `[x]`）；空的小节可删掉标题。追溯完成时间请查 Git。维护约定见 `docs/DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

**结构**：

- **§ P0–P5**：按全局优先级排列的共识项（安全、产品协议、可观测性、测试、运维等）。
- **§ 按模块的优先选项**：按功能域（`agent/`、`llm/`、`tools/` 等）拆分的**中长期**方向，每域若干条；与上文可能交叉（如「多轮会话」在 P1 与多域同时出现），实现后删除已覆盖条目即可。

---

## P0 — 安全（非本机部署前建议处理）

- [ ] **HTTP 无鉴权**：`/chat`、`/chat/stream`、工作区、文件、上传、任务等均未校验调用方身份；`API_KEY` 仅用于调模型，不能防止他人滥用接口与配额。

---

## P1 — 产品 / 协议

- [ ] **`conversation_id` 仅进程内存态**：当前会话存储在进程内 `HashMap`，重启即丢，且多副本无法共享；若需稳定连续会话，需引入外部存储并补齐会话回收/一致性策略。

---

## P2 — 可观测性 / 边角

- [ ] **`mpsc::send` 大量 `let _ =`**：通道关闭或满时静默丢弃（SSE/协议行等）。可在 debug 日志或 metrics 中记录，关键路径可考虑显式错误处理。

---

## P3 — 架构（PER）与文档澄清

- [ ] **终答「反思」深化（可选）**：在已有「`layer_count` ↔ `steps` 条数」规则之外，若要对描述文本与节点/工具结果做更强语义一致校验，可考虑二次 LLM 或更细规则（成本与产品边界需定案）。

---

## P4 — 测试与质量

- [ ] **集成/契约测试**：在 `lib_smoke` 之外，可为 `plan_artifact` 边界、`classify_agent_sse_line` 协议行、`workflow_reflection_controller` 状态迁移增加 fixture 或快照用例。
- [ ] **`stream_chat` 非流式**：可选 wiremock / 静态 JSON fixture 测 `ChatResponse` 解析。
- [ ] **Agent Benchmark 测评与基线**：在主流 agent benchmark（SWE-bench、HumanEval、GAIA 等）上对 CrabMate 做系统性评估，建立能力基线与回归对照，覆盖工具调用、多步推理、代码生成等；批量测评框架已具备（`--benchmark` + `--batch`，支持 SWE-bench / GAIA / HumanEval / Generic），后续在实际数据集上跑通完整流程并记录基线分数、持续追踪迭代。

---

## P5 — 运维与体验

- [ ] **跨进程 / 多副本队列**：当前为**单进程**内 `mpsc` + `Semaphore`；水平扩展需 Redis/SQS 等外部代理与持久化，本仓库未实现。
- [ ] **限流 / 配额**：对 `/chat`、`/chat/stream` 按 IP 或 token 限流（常与 P0 鉴权一起做）。
- [ ] **健康检查扩展**：`health`/`status` 可选增加模型连通性探测（注意成本与频率）。
- [ ] **日志关联**：多轮会话落地后，可统一 `request_id` / `conversation_id`（依赖 P1 会话模型）。

---

## 按模块的优先选项（中长期）

以下为按代码域梳理的 **4～5 条/域**方向，供排期参考；**职责摘要**便于新人定位模块。

### `agent/`（回合编排、上下文、PER、工作流）

**职责摘要**：`agent_turn` 主循环；`context_window` 裁剪/摘要；`per_coord` / `plan_artifact` / `workflow_reflection_controller`；`workflow` DAG 执行。

- [ ] **取消与资源边界**：统一「用户取消 / 队列丢弃 / 工具超时」下的状态机与 SSE 收尾，减少静默丢事件（与 P2、`mpsc` 债呼应）。
- [ ] **规划器/执行器阶段 2（模型与预算解耦）**：在阶段 1 逻辑双 agent 基础上，为 planner / executor 提供独立模型、温度、max_tokens 与上下文预算，建立成本/时延对照基线。
- [ ] **规划器/执行器阶段 3（物理拆分可选）**：评估是否拆分为独立进程/服务（队列、会话与重试语义一致），目标是故障隔离与独立扩缩容；若收益不足则保留同进程架构。
- [ ] **规划与反思策略可插拔**：在现有 `FinalPlanRequirementMode` 之上，允许按场景关闭 PER、或接入轻量规则/二次模型校验（成本可控、可配置）。
- [ ] **工作流可调试性**：DAG 执行轨迹导出、失败节点重试策略、与 `workflow_reflection` 日志字段对齐。
- [ ] **测试与回归基线**：对 `plan_artifact` 解析边界、`run_staged_plan_then_execute_steps` 与 `context_window` 组合增加 fixture/快照测试（与 P4 可合并实现）。

### `llm/` 与 `http_client.rs`（模型请求、重试、流式解析）

**职责摘要**：`ChatRequest` 构造、`complete_chat_retrying`；`api` 中 SSE/JSON 解析；共享 `reqwest::Client` 连接池与超时。

- [ ] **上游错误与限流分类**：区分可重试（429/5xx）与不可重试（401/400），与 `redact` 配合避免日志泄露，可选暴露指标。
- [ ] **Token / 费用预估（可选）**：调用前按消息粗算 token，与 `context_window` 预算联动。
- [ ] **非流式与流式一致性测试**：为 `stream: false` 路径补充契约测试（与 P4 同向）。
- [ ] **可插拔 API 基座**：抽象「兼容 OpenAI Chat Completions」的最小 trait，便于切换自建网关或其它厂商（须遵守密钥与日志规范）。
- [ ] **连接与 TLS 可观测**：可选 debug 级别记录连接复用、首字节延迟（不含敏感 URL 全量）。

### `tools/` 与 `tool_registry.rs`（工具实现与分发策略）

**职责摘要**：表驱动 `ToolSpec`、`run_tool`；`tool_registry` 中 Workflow / 阻塞超时 / 搜索等策略。

- [ ] **危险操作分级与确认**：在 `run_command` / 写文件 / `workflow_execute` 等路径上强化策略（与 P1 Web 审批对齐）。
- [ ] **并行工具调用**：模型一次返回多 `tool_calls` 时，评估依赖关系后安全并行。
- [ ] **工具结果「可引用」摘要**：统一长输出结构化摘要进入 `tool_result.summary`，减少上下文膨胀。
- [ ] **新栈工具按需扩展**：在 `dev_tag` 体系下增加 JVM、容器等标签与最小工具集（保持白名单与路径安全）。Go 已有 `go_build`/`go_test`/`go_vet`/`go_mod_tidy`/`go_fmt_check`/`golangci_lint`；Node.js 已有 `npm_install`/`npm_run`/`npx_run`/`tsc_check`。
- [ ] **registry 策略配置化**：超时、spawn_blocking 类别、`http_fetch` 等更多迁入 `AgentConfig`。

### `sse/`（协议与行分类）

**职责摘要**：`protocol` 编码控制面 JSON；`line` 供 Rust 侧行分类（与 `frontend/src/api.ts` 语义对齐）。

- [ ] **协议版本演进**：`SSE_PROTOCOL_VERSION` bump 时的双端兼容与特性协商（前端分支解析）。
- [ ] **断线重连（可选）**：`Last-Event-ID` 或自定义游标，配合浏览器端重试。
- [ ] **调试/运维事件**：不脱敏前提下可关闭的 `debug` 类 payload（阶段名、耗时等），仅开发模式启用。
- [ ] **与 TypeScript 类型同源**：减少手写 `api.ts` 与 Rust 结构体漂移（生成或共享契约测试）。
- [ ] **错误码全集文档化**：`error.code` 与 HTTP 状态在 `DEVELOPMENT`/`README` 可查。

### `lib.rs` 路由、`chat_job_queue.rs`、`web/`（HTTP 接入与工作区 API）

**职责摘要**：Axum `Router`、`AppState`；对话队列；`web/workspace`、`web/task` 等。

- [ ] **鉴权与多租户隔离**：API Key / Bearer / 反向代理信任头（与 P0 同向）。
- [ ] **`workspace_set` 根路径约束**：仅允许配置白名单根目录内，校验存在性与 symlink 风险（与 P0 同向）。
- [ ] **会话与消息 API**：`messages` 或 `conversation_id` + 存储，与 `run_agent_turn` 对齐（与 P1 同向）。
- [ ] **上传配额与清理策略**：`/upload` 大小、类型、保留时间、按用户或 IP 限额。
- [ ] **OpenAPI / 机器可读契约**：为前端与集成方提供可生成的路由与 body 说明（可选 `utoipa` 等）。

### `config/`（配置加载与 CLI）

**职责摘要**：嵌入/文件 TOML、环境变量、`cli` 参数合并为 `AgentConfig`。

- [ ] **配置校验与友好错误**：启动时报告未知键、类型错误、越界数值，减少静默回退默认值。
- [ ] **热重载（可选）**：仅对安全子集（工具开关、日志级别等）支持 SIGHUP 或文件 watch。
- [ ] **多 profile**：`dev` / `prod` 预设（工具白名单、审批模式、`http_fetch` 前缀等）。
- [ ] **密钥外置**：与密钥管理（vault、文件权限）集成，文档化兼容路径。

### `runtime/`（CLI、会话与导出）

**职责摘要**：`cli`；`workspace_session`、`chat_export`、`message_display`、终端着色与无 SSE 回显。

- [ ] **能力对齐**：Web 有的会话持久、审批、导出格式，在 CLI 有等价或明确「不支持」说明。
- [ ] **（未来）全屏终端 UI**：若重新引入 TUI，需恢复消息列表滚动/缓存与 SSE 行分类消费路径。
- [ ] **REPL 历史与脚本**：可选持久化输入历史、从文件批量注入用户消息。
- [ ] **导出格式版本号**：`chat_export` 与前端导出 JSON 带 schema 版本。
- [ ] **无障碍与终端兼容**：弱终端、配色盲、宽字符与剪贴板失败时的降级提示。

### `frontend/`（Web UI）

**职责摘要**：`api.ts`、各 Panel 组件、`sessionStore`、`chatExport` 等。

- [ ] **浏览器侧多轮状态**：与后端会话 API 同步，刷新不丢、可选加密本地缓存（与 P1 同向）。
- [ ] **聊天列表虚拟化**：极长对话下减少 DOM 与重渲染。
- [ ] **国际化与可访问性**：文案抽取、键盘导航、对比度与焦点管理。
- [ ] **E2E / 契约测试**：关键路径（发消息、工具卡片、工作区设置）用 Playwright 或轻量 stub。

### 横切（`types`、`tool_result`、`health`、`redact`、`text_sanitize`）

**职责摘要**：OpenAI 兼容类型；工具结构化结果；`/health`；日志脱敏；展示层清洗。

- [ ] **请求关联 ID**：自 `lib` 入口生成 `request_id`，贯穿日志与 SSE（与 P5「日志关联」同向）。
- [ ] **健康与容量维度扩展**：在 P5「健康检查扩展」基础上，可选补充磁盘、队列深度等（实现时可合并为少数指标项）。
- [ ] **统一 ToolResult 演进**：版本字段与迁移策略，避免前端与 CLI 解析分叉。
- [ ] **敏感信息规则库**：`redact` 与工具输出截断策略集中维护，新增工具时 checklist。

---

*说明：已完成工作不再写入本文件；必要时查 Git 提交记录。调整 `src/` 模块边界时同步更新 `docs/DEVELOPMENT.md`（见 `.cursor/rules/architecture-docs-sync.mdc`）。安全敏感面见 `.cursor/rules/security-sensitive-surface.mdc`。*
