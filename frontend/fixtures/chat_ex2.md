## 用户

你是谁

## 助手

用户问"你是谁"。这是一个简单的自我介绍问题，不需要调用任何工具。我需要按照要求生成一个合法的 JSON 对象来回应。

根据规则，用户没有提出需要分步执行的具体任务，所以应该设置 `"no_task": true`，并且 `"steps"` 为空数组。

让我构建 JSON 对象：
- type: "agent_reply_plan"
- version: 1
- no_task: true
- steps: []


```json
{
  "type": "agent_reply_plan",
  "version": 1,
  "no_task": true,
  "steps": []
}
```

## 助手

The user is asking "你是谁" (Who are you?). I should respond as the CrabMate programming assistant, introducing myself based on the system prompt.

我是 **CrabMate 编程开发向助手**，一个基于 Rust 构建的 AI Agent，专门协助软件工程任务。

**核心能力**

- 读改代码（Rust / TypeScript / Shell 等）、构建、排错、小范围设计与实现
- 调用 GitHub API、文件搜索、代码分析、格式化与 lint
- 运行测试、构建项目、检查依赖安全

**技术栈**

- 后端：Rust (Axum Web 服务、Leptos 前端)
- 主要 crate：`crabmate`、`crabmate-sse-protocol`、`crabmate-web-leptos`
- 遵循 Conventional Commits（中英双语 commit message）

**我的工作方式**

- 优先用工具而非猜测——读取文件、搜索代码、执行命令来核实状态
- 改动保持最小化，说明改动范围与行为变化
- 遵守仓库约定（`cargo clippy -D warnings`、`cargo fmt`、pre-commit hooks 等）

有什么可以帮你的？

