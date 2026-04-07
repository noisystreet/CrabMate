## 摘要

<!-- 用一两句话说明本 PR 做什么 -->

## 自检清单

请勾选已完成的项（不适用的标为 N/A 并简短说明）：

- [ ] 已在仓库根目录运行 **`pre-commit run --all-files`**（或等价的 `cargo fmt` + **`cargo clippy --all-targets --all-features -- -D warnings`**），且通过
- [ ] 若改动 **`src/`** 或共享契约：已按 **`.cursor/rules/rust-clippy-and-tests.mdc`** 运行相应范围的 **`cargo test`**
- [ ] 若改动 **`frontend-leptos/`**：已运行 **`cd frontend-leptos && cargo check --target wasm32-unknown-unknown`**（大改时 **`trunk build`** 或发版路径 **`trunk build --release`**）
- [ ] 若改动聊天 / SSE / **`frontend-leptos/src/api.rs`**：已核对双端协议一致（见 **`.cursor/rules/api-sse-chat-protocol.mdc`**）
- [ ] 若改动 **`config/default_config.toml`**、**`config/session.toml`**、**`config/context_inject.toml`**、**`config/tools.toml`**、**`config/sandbox.toml`**、**`config/planning.toml`**、**`config/memory.toml`**、**`config.toml.example`**、**`AGENT_*` 环境变量** 或 Axum 路由契约：已更新 **`README.md`** 与 **`docs/DEVELOPMENT.md`**
- [ ] 若改动架构或 **`src/` 模块组织**：已更新 **`docs/DEVELOPMENT.md`**（见 **`.cursor/rules/architecture-docs-sync.mdc`**）
- [ ] 无真实密钥、token、私钥进入 diff；日志与错误信息已脱敏（见 **`.cursor/rules/secrets-and-logging.mdc`**）

## 相关 Issue

<!-- 例如 Closes #123，无则写 N/A -->
