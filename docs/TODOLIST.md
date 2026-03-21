# 后续修复与完善清单

本文记录代码审查与架构讨论中积累的待办，按优先级排序，便于分批实施。完成某项后可将对应条目勾掉或删除。

---

## P0 — 安全（非本机部署前建议处理）

- [ ] **HTTP 无鉴权**：`/chat`、`/chat/stream`、工作区、文件、上传、任务等均未校验调用方身份；`API_KEY` 仅用于调模型，不能防止他人滥用接口与配额。
- [ ] **默认监听 `0.0.0.0`**：`src/lib.rs` 中 Web 服务绑定全网卡；公网/局域网暴露时与上一条叠加风险高。可考虑默认 `127.0.0.1`、或通过 CLI/配置显式开启 `0.0.0.0`。
- [ ] **`workspace_set` 任意路径**：`src/ui/workspace.rs::workspace_set_handler` 直接写入 `workspace_override`，未校验路径存在性、是否落在允许根目录内、或敏感路径黑名单。攻击面：在进程权限内将工作区指向任意目录，再配合 Agent 工具与文件 API。

---

## P1 — 产品 / 协议

- [ ] **Web 聊天无多轮历史**：`ChatRequestBody` 仅 `message: String`（`src/lib.rs`），`max_message_history` 截断在当前 API 下几乎不生效。若需会话延续，需扩展请求体（如 `messages` / `conversation_id` + 服务端存储）并统一与 `run_agent_turn` 对齐。
- [ ] **Web 侧 workflow 审批（可选）**：当前 TUI 可对命令/workflow 做人机审批，Web 为 `NoApproval`。若要对齐安全模型，需产品定案 + SSE/前端确认流 + 超时默认拒绝。

---

## P2 — 健壮性

- [ ] **`tool_registry` 中 `unreachable!`**：`RunExecutable` 分支假定 TUI 已 remap；若注册表或调用路径变更，可能运行时 panic。可改为返回明确错误字符串或 `Result`。
- [ ] **非流式 `stream_chat` 响应形态**：`no_stream` 路径依赖 OpenAI 形 `choices[0].message` JSON；上游字段差异会导致反序列化失败。可考虑宽松反序列化、错误信息中带响应片段（已有部分）、或文档标明仅保证 DeepSeek/OpenAI 兼容实现。
- [ ] **`tool_registry` 元数据与 `dead_code`**：部分类型仅单测引用导致告警；可 `#[allow(dead_code)]`、或接到真实能力（如 `/status` 暴露已注册工具列表）。

---

## P3 — 可观测性 / 边角

- [ ] **`mpsc::send` 大量 `let _ =`**：通道关闭或满时静默丢弃（TUI/SSE/协议行）。可在 debug 日志或 metrics 中记录，关键路径（如回合结束 `sync_tx`）可考虑显式错误处理。
- [ ] **文档与运行说明**：在 `README.md` 中明确「默认监听地址、无鉴权、仅供可信环境」等假设，避免误部署。

---

## P4 — 架构（PER）与文档澄清

- [ ] **终答「反思」深化（可选）**：`after_final_assistant` 目前只校验 `agent_reply_plan` JSON 是否存在，不校验步骤是否覆盖刚执行的 workflow/工具结果。若要做「语义一致」反思，需规则或二次 LLM（成本与产品边界需定案）。
- [ ] **主循环命名/文档**：`per_plan_call_model_retrying` 实为「本轮 LLM 调用」而非独立规划器；在 `agent_turn` 注释或 `DEVELOPMENT.md` 写明 **P = 模型产出（含 tool_calls）**，降低误读。
- [ ] **强制规划的触发策略**：除 `workflow_reflection_plan_next` 外，其它场景若也要终答带规划，需配置或工具元数据驱动，避免隐式耦合。

---

## P5 — 前端与 SSE 协议

- [ ] **控制面事件消费对齐**：核对 `sse_protocol`（含 `v`、`plan_required`、结构化 `ToolResult` 等）与 `frontend/src/api.ts` 是否全覆盖；约定 `v` 递增时的兼容策略并写入 `DEVELOPMENT.md`。

---

## P6 — 代码组织与工程债

- [ ] **`runtime/tui.rs` 拆分**：状态机、绘制、Agent 桥接可分子模块（如 `tui/state.rs`、`draw.rs`），便于测试与维护。
- [ ] **生产路径 `panic`/`expect` 扫描**：除 `unreachable!` 外，对 `src/` 非测试代码做一轮审计。

---

## P7 — 测试与质量

- [ ] **集成/契约测试**：在 `lib_smoke` 之外，可为 `plan_artifact` 边界、`classify_agent_sse_line` 协议行、`workflow_reflection_controller` 状态迁移增加 fixture 或快照用例。
- [ ] **`stream_chat` 非流式**：可选 wiremock / 静态 JSON fixture 测 `ChatResponse` 解析。

---

## P8 — 运维与体验

- [ ] **限流 / 配额**：对 `/chat`、`/chat/stream` 按 IP 或 token 限流（常与 P0 鉴权一起做）。
- [ ] **健康检查扩展**：`health`/`status` 可选增加模型连通性探测（注意成本与频率）。
- [ ] **日志关联**：多轮会话落地后，可统一 `request_id` / `conversation_id`（依赖 P1 会话模型）。

---

## 已完成的上下文（无需再列入本清单）

- TUI：`state.messages` 与后台回合结束同步、`ToolCall`/`ToolResult` 不污染正文（历史改动）。
- CLI `--no-stream` 与 `stream_chat` 的 `stream: false` 接线（历史改动）。
- `.gitignore`：`/target/`、仓库根 `.crabmate/`。

---

*最后更新：合并「可完善项」讨论与既有安全/协议待办。*
