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

---

## P2 — 健壮性

- [ ] **`tool_registry` 中 `unreachable!`**：`RunExecutable` 分支假定 TUI 已 remap；若注册表或调用路径变更，可能运行时 panic。可改为返回明确错误字符串或 `Result`。
- [ ] **非流式 `stream_chat` 响应形态**：`no_stream` 路径依赖 OpenAI 形 `choices[0].message` JSON；上游字段差异会导致反序列化失败。可考虑宽松反序列化、错误信息中带响应片段（已有部分）、或文档标明仅保证 DeepSeek/OpenAI 兼容实现。

---

## P3 — 可观测性 / 边角

- [ ] **`mpsc::send` 大量 `let _ =`**：通道关闭或满时静默丢弃（TUI/SSE/协议行）。可在 debug 日志或 metrics 中记录，关键路径（如回合结束 `sync_tx`）可考虑显式错误处理。
- [ ] **文档与运行说明**：在 `README.md` 中明确「默认监听地址、无鉴权、仅供可信环境」等假设，避免误部署。

---

## 已完成的上下文（无需再列入本清单）

- TUI：`state.messages` 与后台回合结束同步、`ToolCall`/`ToolResult` 不污染正文（历史改动）。
- CLI `--no-stream` 与 `stream_chat` 的 `stream: false` 接线（历史改动）。

---

*最后更新：按「严重问题」审查会话整理。*
