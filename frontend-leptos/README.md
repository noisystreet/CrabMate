# CrabMate Web（Leptos + WASM，实验性）

与根目录 `frontend/`（Vite + React）并行；`cargo run -- serve` 在 **`frontend-leptos/dist` 存在时优先**提供该目录，否则使用 `frontend/dist`。

## 依赖

- Rust **`wasm32-unknown-unknown`**：`rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev/)：`cargo install trunk`（本仓库 CI/开发机已用 0.21.x）

构建时若环境变量 **`NO_COLOR=1`**，部分 Trunk 版本会报错，可先 `unset NO_COLOR` 再执行 `trunk build`。

## 构建

```bash
cd frontend-leptos
trunk build --release
```

产物在 **`frontend-leptos/dist/`**。

## 能力与现状

已覆盖与 React 版相近的**壳层**：顶栏、聊天列表 + 输入框、`POST /chat/stream` SSE、命令审批条、工作区列表、任务清单勾选、本地会话列表（与 React 共用 `localStorage` 键）、深浅主题切换。

**尚未**对齐 React 版的完整能力（示例：Markdown/KaTeX 渲染、虚拟列表、附件上传、会话管理全功能、工作区文件编辑/搜索等）。后续可按需移植 `frontend/src/` 中的逻辑。
