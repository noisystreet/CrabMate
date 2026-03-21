# 后续修复与完善清单

本文仅保留**未完成**待办，按优先级排序。**某项完成后必须从本文件删除该条目**（勿长期保留 `[x]`）；追溯完成时间请查 Git。维护约定见 `docs/DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

---

## P0 — 安全（非本机部署前建议处理）

- [ ] **HTTP 无鉴权**：`/chat`、`/chat/stream`、工作区、文件、上传、任务等均未校验调用方身份；`API_KEY` 仅用于调模型，不能防止他人滥用接口与配额。
- [ ] **默认监听 `0.0.0.0`**：`src/lib.rs` 中 Web 服务绑定全网卡；公网/局域网暴露时与上一条叠加风险高。可考虑默认 `127.0.0.1`、或通过 CLI/配置显式开启 `0.0.0.0`。
- [ ] **`workspace_set` 任意路径**：`src/ui/workspace.rs::workspace_set_handler` 直接写入 `workspace_override`，未校验路径存在性、是否落在允许根目录内、或敏感路径黑名单。攻击面：在进程权限内将工作区指向任意目录，再配合 Agent 工具与文件 API。

---

## P1 — 产品 / 协议

- [ ] **Web 聊天无跨请求多轮历史**：`ChatRequestBody` 仍仅 `message: String`（`src/lib.rs`），服务端不持久化会话；单请求内的多轮工具循环已由 `context_window` 做截断/预算/可选摘要。若需浏览器侧连续对话，需扩展请求体（如 `messages` / `conversation_id` + 存储）并与 `run_agent_turn` 对齐。
- [ ] **Web 侧 workflow 审批（可选）**：当前 TUI 可对命令/workflow 做人机审批，Web 为 `NoApproval`。若要对齐安全模型，需产品定案 + SSE/前端确认流 + 超时默认拒绝。

---

## P2 — 可观测性 / 边角

- [ ] **`mpsc::send` 大量 `let _ =`**：通道关闭或满时静默丢弃（TUI/SSE/协议行）。可在 debug 日志或 metrics 中记录，关键路径（如回合结束 `sync_tx`）可考虑显式错误处理。

---

## P3 — 架构（PER）与文档澄清

- [ ] **终答「反思」深化（可选）**：在已有「`layer_count` ↔ `steps` 条数」规则之外，若要对描述文本与节点/工具结果做更强语义一致校验，可考虑二次 LLM 或更细规则（成本与产品边界需定案）。

---

## P4 — 测试与质量

- [ ] **集成/契约测试**：在 `lib_smoke` 之外，可为 `plan_artifact` 边界、`classify_agent_sse_line` 协议行、`workflow_reflection_controller` 状态迁移增加 fixture 或快照用例。
- [ ] **`stream_chat` 非流式**：可选 wiremock / 静态 JSON fixture 测 `ChatResponse` 解析。

---

## P5 — 运维与体验

- [ ] **限流 / 配额**：对 `/chat`、`/chat/stream` 按 IP 或 token 限流（常与 P0 鉴权一起做）。
- [ ] **健康检查扩展**：`health`/`status` 可选增加模型连通性探测（注意成本与频率）。
- [ ] **日志关联**：多轮会话落地后，可统一 `request_id` / `conversation_id`（依赖 P1 会话模型）。

---

*说明：已完成工作不再写入本文件；必要时查 Git 提交记录。*
