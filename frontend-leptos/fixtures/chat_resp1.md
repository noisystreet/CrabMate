<redacted_thinking>
用户问"你会哪些技能"，这是一个闲聊/介绍类问题，没有具体任务需要执行。

按照要求，我需要：
1. 不调用任何工具
2. 在回复中用 JSON 代码围栏给出规划对象
3. 因为没有具体任务，"no_task" 应为 true，"steps" 为空数组

让我写一个简洁的回答，介绍 CrabMate 的主要能力。
</redacted_thinking>

你好！我是 **CrabMate 编程开发向助手**，主要技能如下：

## 代码与工程

- **代码阅读与修改**：定位符号/引用、读写文件、小范围重构
- **Bug 排查**：根据报错/日志复现问题、定位根因、修复
- **构建与测试**：运行 `cargo build/test/clippy`、`trunk build` 等
- **代码审查**：按仓库规范（`cargo fmt`、clippy、pre-commit）检查
- **Git 操作**：commit（符合 Conventional Commits）、rebase、push（含安全检查）

## 架构与文档

- **模块索引**：读 `README.md`、`docs/DEVELOPMENT.md` 等了解技术栈与调用链
- **协议一致**：修改 SSE/HTTP 协议时同步前后端与文档（见 `api-sse-chat-protocol.mdc`）
- **架构同步**：模块增删时更新 `docs/DEVELOPMENT.md`

## 安全与合规

- **密钥脱敏**：不泄露 API Key，不提交敏感信息
- **依赖审计**：`cargo audit` / `cargodeny check licenses`
- **安全敏感面自检**：路径穿越、命令执行白名单、HTTP拉取前缀等

## 工具调用原则

- **先定位再动手**：用 `search_in_files` 找代码，避免臆测
- **最小改动**：保持对外行为一致，格式/风格对齐已有代码
- **验证优先**：改后跑相关测试或检查，不跳过自动化验证

---

```json
{
"type": "agent_reply_plan",
"version":1,
"no_task": true,
"steps": []
}
```

有具体代码任务或问题可以直接提，我来帮你分析或修改。<redacted_thinking>
用户发送了"你会哪些技能"，这是一个闲聊/介绍类问题。我已经按照要求用 JSON 代码围栏给出了规划对象，并且因为没有具体任务，"no_task" 为 true，"steps" 为空数组。

回复已经完成，等待用户下一步指令。
</redacted_thinking>

好的，我可以帮你做代码相关的任务，比如定位符号、修改代码、排查 bug、跑测试等。有需要的话直接说！

