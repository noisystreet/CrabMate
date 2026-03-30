**语言 / Languages:** 中文（本页）· [English](en/TODOLIST.md)

# 后续修复与完善清单

本文仅保留**未完成**待办。**某项完成后必须从本文件删除该条目**（勿长期保留 `[x]`）；空的小节可删掉标题。追溯完成时间请查 Git。维护约定见 `docs/DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

**结构**：

- **全局优先级（跨模块）**：按 **P0–P5** 共识项（安全、架构、测试、运维等）；与模块章可能交叉，实现后删除即可。
- **按模块分章**：每个功能域单独一章（`agent/`、`llm/`、`tools/` 等），章首**职责摘要**便于定位；条目前可含与全局段的交叉引用。

---

## 全局优先级（跨模块）

### P0 — 安全（非本机部署前建议处理）

- [ ] **HTTP 无鉴权**：`/chat`、`/chat/stream`、工作区、文件、上传、任务等均未校验调用方身份；`API_KEY` 仅用于调模型，不能防止他人滥用接口与配额。
- [ ] **多角色与人设切换**：支持创建/配置多种**角色**（各角色可绑定不同系统提示、工具可见性、温度等策略）；**CLI**（如 `repl` 的 `/` 命令）与 **Web** 提供**内建命令或等价控件**切换当前会话生效角色，且与会话持久化、导出、`POST /config/reload` 可重载字段的边界在 `README` / `docs/DEVELOPMENT.md` 中写清；若对多租户开放须与上条鉴权同盘。

### P3 — 架构（PER）与文档澄清

- [ ] **终答「反思」深化（可选）**：在已有「`layer_count` ↔ `steps` 条数」规则之外，若要对描述文本与节点/工具结果做更强语义一致校验，可考虑二次 LLM 或更细规则（成本与产品边界需定案）。

### P4 — 测试与质量

- [ ] **错误类型统一（渐进）**：生产路径减少纯 `String` / `format!` 导致无法区分「不允许 / 未找到 / 非零退出」等；优先为 `run_command`（如 `tools/command.rs`）与路径解析等热点引入可判别错误类型；`path_workspace` 等逐步由 `Result<_, String>` 迁向结构化枚举（与安全边界、日志分类同向）。
- [ ] **生产路径 unwrap/expect 审计**：梳理非测试代码中的 `unwrap` / `expect`（如 `per_coord`、`conversation_store`、命令退出码处理），改为显式传播或带业务上下文的 `expect`，降低低概率 panic 与排障成本。
- [ ] **集成/契约测试**：在 `lib_smoke` 与 **`tests/cli_contract.rs`**（`parse_args_from_argv`、`normalize_legacy_argv` fixture、`classify_model_error_message` / `EXIT_*`）之外，可为 `plan_artifact` 边界、`classify_agent_sse_line` 协议行、`workflow_reflection_controller` 状态迁移增加 fixture 或快照用例。
- [ ] **`stream_chat` 非流式**：可选 wiremock / 静态 JSON fixture 测 `ChatResponse` 解析。
- [ ] **Agent Benchmark 测评与基线**：在主流 agent benchmark（SWE-bench、HumanEval、GAIA 等）上对 CrabMate 做系统性评估，建立能力基线与回归对照，覆盖工具调用、多步推理、代码生成等；批量测评框架已具备（`--benchmark` + `--batch`，支持 SWE-bench / GAIA / HumanEval / Generic），后续在实际数据集上跑通完整流程并记录基线分数、持续追踪迭代。

### P5 — 运维与体验

- [ ] **跨进程 / 多副本队列**：当前为**单进程**内 `mpsc` + `Semaphore`；水平扩展需 Redis/SQS 等外部代理与持久化，本仓库未实现。
- [ ] **限流 / 配额**：对 `/chat`、`/chat/stream` 按 IP 或 token 限流（常与 P0 鉴权一起做）。
- [ ] **健康检查扩展**：`health`/`status` 可选增加模型连通性探测（注意成本与频率）。
- [ ] **日志关联**：多轮会话落地后，可统一 `request_id` / `conversation_id`（依赖 P1 会话模型）。

---

## `agent/`（回合编排、上下文、PER、工作流）

**职责摘要**：`agent_turn` 主循环；`context_window` 裁剪/摘要；`per_coord` / `plan_artifact` / `workflow_reflection_controller`；`workflow` DAG 执行。

- [ ] **agent_turn 与 llm 职责边界**：明确 `llm` 偏协议与单次调用、`agent_turn` 偏编排与回合状态，减少「重试/流式」等感知分散；`per_coord` 若同时承载工作流反思与规划重写，可评估抽出 `reflection` 子模块或在文档中固化状态机边界。
- [ ] **规划器/执行器阶段 2（模型与预算解耦）**：在阶段 1 逻辑双 agent 基础上，为 planner / executor 提供独立模型、温度、max_tokens 与上下文预算，建立成本/时延对照基线。
- [ ] **规划器/执行器阶段 3（物理拆分可选）**：评估是否拆分为独立进程/服务（队列、会话与重试语义一致），目标是故障隔离与独立扩缩容；若收益不足则保留同进程架构。
- [ ] **规划与反思策略可插拔**：在现有 `FinalPlanRequirementMode` 之上，允许按场景关闭 PER、或接入轻量规则/二次模型校验（成本可控、可配置）。
- [ ] **测试与回归基线**：对 `plan_artifact` 解析边界、`run_staged_plan_then_execute_steps` 与 `context_window` 组合增加 fixture/快照测试（与 P4 可合并实现）。
- [ ] **长期记忆：外部向量库与多租户**：当前已支持会话级 SQLite + 可选本地 **fastembed** 检索；后续可接 **Qdrant** / **pgvector**、跨会话租户键、与 P0 鉴权强绑定及更细 `redact` 策略。
- [ ] **长期记忆：检索质量**：混合检索（如 SQLite **FTS5** 与向量分数加权）、多轮上下文或会话主题感知的 **query** 构造，降低仅依赖末条 `user` 的漂移与漏召。
- [ ] **长期记忆：条目生命周期与去重**：**TTL**、访问频率或显式 **pin**；语义分块与近重复合并/淘汰策略，避免 `max_entries` 仅按时间删旧导致有效事实被挤出。
- [ ] **长期记忆：写入与索引策略**：选择性索引（跳过过短轮次、纯工具调试等）、同步索引与失败重试/补偿可配置；索引耗时与错误在可观测面汇总。
- [ ] **长期记忆：运营与合规 API**：在 **P0 鉴权**落地前提下，提供只读列表/按 scope 删除等管理接口（排障与「被遗忘权」类需求）；`/status` 可暴露条目规模、embed 降级次数等（不脱敏全文）。
- [ ] **长期记忆：Web 与文档体验**：侧栏或面板展示「本轮注入摘要」、单条忽略/置顶；在 UI/README 中区分**长期记忆**、**工作区备忘文件**与**项目画像**的职责边界。
- [ ] **工作区统一代码索引（持久 + 增量）**：全仓源码与元数据索引以加速浏览/检索；与 `ReadFileTurnCache`、长期记忆 store 分离；路径安全与 P0 多租户同向。分阶段与验收见 **`docs/CODEBASE_INDEX_PLAN.md`**。

---

## `llm/` 与 `http_client.rs`（模型请求、重试、流式解析）

**职责摘要**：`ChatRequest` 构造、`complete_chat_retrying`；`api` 中 SSE/JSON 解析；共享 `reqwest::Client` 连接池与超时。

- [ ] **LLM 上游指标（可选）**：按 HTTP 状态/可重试维度暴露计数或对接外部 metrics（当前错误日志已含 `http_status` / `retryable` 字段）。
- [ ] **Token / 费用预估（可选）**：调用前按消息粗算 token，与 `context_window` 预算联动。
- [ ] **非流式与流式一致性测试**：为 `stream: false` 路径补充契约测试（与 P4 同向）。
- [ ] **连接与 TLS 可观测**：可选 debug 级别记录连接复用、首字节延迟（不含敏感 URL 全量）。

---

## `tools/` 与 `tool_registry.rs`（工具实现与分发策略）

**职责摘要**：表驱动 `ToolSpec`、`run_tool`；`tool_registry` 中 Workflow / 阻塞超时 / 搜索等策略。

- [ ] **危险操作分级与确认（续）**：写盘类等工具若需审批或细粒度策略，扩展 [`tool_approval::SensitiveCapability`] 与配置项（当前 `run_command` / `http_fetch` / `http_request` / 工作流审批已统一经 **`tool_approval`**）。
- [ ] **新栈工具按需扩展**：在 `dev_tag` 体系下按需增加其它语言栈标签与最小工具集（保持白名单与路径安全）。Go 已有 `go_build`/`go_test`/`go_vet`/`go_mod_tidy`/`go_fmt_check`/`golangci_lint`；JVM 已有 `maven_*`/`gradle_*`；容器已有 `docker_*`/`podman_images`；Node.js 已有 `npm_install`/`npm_run`/`npx_run`/`tsc_check`。
- [ ] **registry 策略配置化**：超时、spawn_blocking 类别、`http_fetch` 等更多迁入 `AgentConfig`。
- [ ] **MCP 扩展**：可选将本 agent 以 MCP server 暴露；客户端支持 Streamable HTTP / SSE、鉴权与多 server；与 `run_command` / 工作区策略的边界在文档中细化。

---

## `sse/`（协议与行分类）

**职责摘要**：`protocol` 编码控制面 JSON；`line` 供 Rust 侧行分类（与 `frontend/src/api.ts` 语义对齐）。

- [ ] **协议版本演进**：`SSE_PROTOCOL_VERSION` bump 时的双端兼容与特性协商（前端分支解析）。
- [ ] **断线重连（可选）**：`Last-Event-ID` 或自定义游标，配合浏览器端重试。
- [ ] **调试/运维事件**：不脱敏前提下可关闭的 `debug` 类 payload（阶段名、耗时等），仅开发模式启用。
- [ ] **与 TypeScript 类型同源**：减少手写 `api.ts` 与 Rust 结构体漂移（生成或共享契约测试）。
- [ ] **错误码全集文档化**：`error.code` 与 HTTP 状态在 `DEVELOPMENT`/`README` 可查。

---

## `lib.rs` 路由、`chat_job_queue.rs`、`web/`（HTTP 接入与工作区 API）

**职责摘要**：Axum `Router`、`AppState`；对话队列；`web/workspace`、`web/task` 等。

- [ ] **鉴权与多租户隔离**：API Key / Bearer / 反向代理信任头（与 P0 同向）。
- [ ] **会话与消息 API**：`messages` 或 `conversation_id` + 存储，与 `run_agent_turn` 对齐（与 P1 同向）。
- [ ] **上传配额与清理策略**：`/upload` 大小、类型、保留时间、按用户或 IP 限额。
- [ ] **OpenAPI / 机器可读契约**：为前端与集成方提供可生成的路由与 body 说明（可选 `utoipa` 等）。

---

## `config/`（配置加载与 CLI）

**职责摘要**：嵌入/文件 TOML、环境变量、`cli` 参数合并为 `AgentConfig`。

- [ ] **按域子配置与装配收口**：评估将 `ConfigBuilder` / `AgentConfig` 装配按 LLM、工具、Web 等域拆子结构，集中「嵌入默认 → 文件 → 环境变量」覆盖顺序与校验入口，缓解字段膨胀导致的遗漏与依赖不透明（模型专有开关仅对部分 `model` 生效等须在文档与校验中可发现）。
- [ ] **配置校验与友好错误**：启动时报告未知键、类型错误、越界数值，减少静默回退默认值。
- [ ] **热重载（可选）**：仅对安全子集（工具开关、日志级别等）支持 SIGHUP 或文件 watch。
- [ ] **多 profile**：`dev` / `prod` 预设（工具白名单、审批模式、`http_fetch` 前缀等）。
- [ ] **密钥外置**：与密钥管理（vault、文件权限）集成，文档化兼容路径。

---

## `runtime/`（CLI、会话与导出）

**职责摘要**：`cli`；`workspace_session`、`chat_export`、`message_display`、终端着色与无 SSE 回显。

- [ ] **（未来）全屏终端 UI**：若重新引入 TUI，需恢复消息列表滚动/缓存与 SSE 行分类消费路径。
- [ ] **CLI 历史与脚本**：可选持久化输入历史、从文件批量注入用户消息。
- [ ] **导出格式版本号**：`chat_export` 与前端导出 JSON 带 schema 版本。
- [ ] **无障碍与终端兼容**：弱终端、配色盲、宽字符与剪贴板失败时的降级提示。

---

## `frontend/`（Web UI）

**职责摘要**：`api.ts`、各 Panel 组件、`sessionStore`、`chatExport` 等。

- [ ] **浏览器侧多轮状态**：与后端会话 API 同步，刷新不丢、可选加密本地缓存（与 P1 同向）。
- [ ] **聊天列表虚拟化**：极长对话下减少 DOM 与重渲染。
- [ ] **国际化与可访问性**：文案抽取、键盘导航、对比度与焦点管理。
- [ ] **E2E / 契约测试**：关键路径（发消息、工具卡片、工作区设置）用 Playwright 或轻量 stub。
- [ ] **语音交互（未来）**：浏览器侧麦克风采集、STT（可对接云端或本地引擎）、TTS 播放；与现有聊天/SSE 流衔接；权限、隐私与错误降级文案；若走后端代理需在 `web/` 增加路由并与鉴权（P0）同盘。

---

## 横切（`types`、`tool_result`、`health`、`redact`、`text_sanitize`）

**职责摘要**：OpenAI 兼容类型；工具结构化结果；`/health`；日志脱敏；展示层清洗。

- [ ] **请求关联 ID**：自 `lib` 入口生成 `request_id`，贯穿日志与 SSE（与 P5「日志关联」同向）。
- [ ] **健康与容量维度扩展**：在 P5「健康检查扩展」基础上，可选补充磁盘、队列深度等（实现时可合并为少数指标项）。
- [ ] **统一 ToolResult 演进（载荷版本 + 迁移 + 契约）**：避免 Web（SSE `tool_result` / `ToolResultBody`）、写入历史的 **`crabmate_tool` 信封**（`tool_result_envelope_v1`）与 **CLI/TUI**（`runtime/message_display.rs` 对 JSON / 纯文本分支）各自演进时语义分叉。
  - **现状锚点**：信封内已有 **`crabmate_tool.v: 1`**（`src/tool_result.rs` 的 `encode_tool_message_envelope_v1` 等）；SSE 为外层 **`SseMessage.v`**（`SSE_PROTOCOL_VERSION`，见 `src/sse/protocol.rs`）+ 扁平 **`tool_result`** 字段；前端 **`frontend/src/api.ts` 的 `ToolResultInfo`** 与 **`frontend/src/sse_control_dispatch.ts` 的 `ToolResultInfoDispatch`** 手写同形，须与 Rust 同步。
  - **版本语义**：明确区分「整条 SSE 控制面版本」与「工具结果载荷版本」；载荷 bump 时同步 **`docs/SSE_PROTOCOL.md`**、**`src/sse/protocol.rs`**、**`tool_result` 信封编解码**、**`execute_tools` 下发字段** 与 **前端解析**；breaking 时按 **`api-sse-chat-protocol.mdc`** 跑 **`golden_sse_control`** / **`verify-sse-contract`** 等。
  - **迁移与读路径收口**：新增/变更字段时优先经 **单一 normalize 层**（按信封 `v` / 可选 SSE 内层版本分支 → 内部统一结构），再供 UI 与导出消费；**纯文本旧输出**仍走 **`parse_legacy_output` / `ToolResult::from_legacy_output`**，避免重复解析逻辑散落在 `message_display` 与 SSE 回调。
  - **契约测试**：在现有 **`fixtures/sse_control_golden.jsonl`** + **`src/sse/control_dispatch_mirror.rs`** 基础上，为 **`crabmate_tool` 各版本样例**（含压缩后的 `output_truncated` 等）补充 golden 或 **`tool_result` 单元测**，防止回放/导出与线上一致性回归。
  - **可选长期**：以 JSON Schema 或 codegen 生成/校验 TS 与 Rust，减少双端字段漂移（与 **`sse/` 章「与 TypeScript 类型同源」** 同向）。
- [ ] **敏感信息规则库**：`redact` 与工具输出截断策略集中维护，新增工具时 checklist。

---

*说明：已完成工作不再写入本文件；必要时查 Git 提交记录。调整 `src/` 模块边界时同步更新 `docs/DEVELOPMENT.md`（见 `.cursor/rules/architecture-docs-sync.mdc`）。安全敏感面见 `.cursor/rules/security-sensitive-surface.mdc`。*
