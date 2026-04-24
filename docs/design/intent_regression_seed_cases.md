# Intent Regression Seed Cases

状态：seed v1（用于 L1/L2 回归与阈值调优）  
说明：每条样本包含「输入」「期望 primary_intent」「期望动作」。

## 1) Read/Inspect（应直接执行）

- 输入：当前目录下有哪些源文件  
  期望：`execute.read_inspect` / `Execute`
- 输入：列出 `src/agent` 目录里的 Rust 文件  
  期望：`execute.read_inspect` / `Execute`
- 输入：帮我看一下 `Cargo.toml` 里依赖  
  期望：`execute.read_inspect` / `Execute`
- 输入：查看最近 5 个提交  
  期望：`execute.read_inspect` / `Execute`
- 输入：这个项目现在有哪些测试文件  
  期望：`execute.read_inspect` / `Execute`

## 2) Debug Diagnose

- 输入：`cargo test` 报错，帮我定位原因  
  期望：`execute.debug_diagnose` / `Execute`
- 输入：这个 panic 是怎么来的，帮我排查  
  期望：`execute.debug_diagnose` / `Execute`
- 输入：线上异常日志里有空指针，先定位  
  期望：`execute.debug_diagnose` / `Execute`

## 3) Run/Test/Build

- 输入：先跑一下 `cargo test`  
  期望：`execute.run_test_build` / `Execute`
- 输入：帮我构建前端并检查是否通过  
  期望：`execute.run_test_build` / `Execute`
- 输入：运行 clippy 并修复 warning  
  期望：`execute.run_test_build` / `Execute`

## 4) Code Change

- 输入：把这个函数重构成更清晰的结构  
  期望：`execute.code_change` / `Execute`
- 输入：实现一个新的配置解析函数  
  期望：`execute.code_change` / `Execute`
- 输入：把这个 if-else 改成 match  
  期望：`execute.code_change` / `ConfirmThenExecute|Execute`

## 5) Docs Ops

- 输入：更新 README 的快速开始章节  
  期望：`execute.docs_ops` / `Execute`
- 输入：补一段 API 使用说明到 docs  
  期望：`execute.docs_ops` / `Execute`

## 6) Git Ops

- 输入：把当前改动提交并写一个 commit message  
  期望：`execute.git_ops` / `Execute|ConfirmThenExecute`
- 输入：开一个 PR，标题用 fix intent routing  
  期望：`execute.git_ops` / `Execute|ConfirmThenExecute`

## 7) Mixed Intents（应有 secondary_intents）

- 输入：先跑测试并修复失败，再提交改动  
  期望：primary=`execute.git_ops`，secondary 含 `execute.run_test_build`、`execute.debug_diagnose`
- 输入：先列出涉及文件，再改代码并更新文档  
  期望：primary=`execute.code_change`，secondary 含 `execute.read_inspect`、`execute.docs_ops`

## 8) QA / Greeting / Ambiguous

- 输入：你能做什么  
  期望：`qa.explain` / `DirectReply`
- 输入：你好  
  期望：`meta.greeting` / `DirectReply`
- 输入：帮我看看  
  期望：`unknown` / `ClarifyThenExecute`

