# CrabMate Web（Leptos + WASM，实验性）

当前项目唯一 Web 前端实现；`cargo run -- serve` 读取 **`frontend-leptos/dist`**。

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

已覆盖当前 Web 端能力：顶栏、聊天列表 + 输入框、`POST /chat/stream` SSE、命令审批条、工作区列表、任务清单勾选、本地会话列表（弹窗内可重命名、删除、下载 **JSON** / **Markdown** 导出，JSON 为 `ChatSessionFile` v1，与 CLI `save-session` 落盘同形）、深浅主题切换。状态栏在 `GET /status` 失败时展示错误说明与「重试」按钮。动效：生成中/工具执行中状态点脉冲、侧栏宽度与显隐过渡、工作区/任务列表加载后行级渐显；`prefers-reduced-motion: reduce` 下关闭上述动画与过渡。
