# CrabMate Web（Leptos + WASM，实验性）

当前项目唯一 Web 前端实现；`cargo run -- serve` 读取 **`frontend-leptos/dist`**。

## 依赖

- Rust **`wasm32-unknown-unknown`**：`rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev/)：`cargo install trunk`（本仓库 CI/开发机已用 0.21.x）

构建时若环境变量 **`NO_COLOR=1`**，部分 Trunk 版本会报错，可先 `unset NO_COLOR` 再执行 `trunk build`。

样式源文件按模块拆在 **`frontend-leptos/styles/*.css`**；**Trunk** 在 **`index.html`** 里对每条模块 CSS 使用 `<link data-trunk rel="css" href="styles/…">` 打入 `dist`（单文件 `@import` 在打包后路径会失效，勿只靠根目录 `styles.css` 聚合）。修改后仍需 `trunk build` 生成 `dist`。发版前视觉手测清单见 **`docs/frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md`**。

## 构建

```bash
cd frontend-leptos
trunk build --release
```

产物在 **`frontend-leptos/dist/`**。

## 能力与现状

已覆盖当前 Web 端能力：顶栏、聊天列表 + 输入框、`POST /chat/stream` SSE、命令审批条、工作区列表、任务清单勾选、本地会话列表（弹窗内可重命名、删除、下载 **JSON** / **Markdown** 导出，JSON 为 `ChatSessionFile` v1，与 CLI `save-session` 落盘同形）。右列工具栏「**设置**」弹窗内可切换深浅主题与是否显示页面背景光晕（`localStorage`：`crabmate-theme`、`crabmate-bg-decor`）。状态栏在 `GET /status` 失败时展示错误说明与「重试」按钮。动效：生成中/工具执行中状态点脉冲、右列宽度与显隐过渡、窄屏左栏抽屉滑动、工作区/任务列表加载后行级渐显；`prefers-reduced-motion: reduce` 下在 `styles/motion.css` 统一关闭相关动画与尺寸/transform 过渡，并显式落到终态以避免布局闪跳（主题切换仅变量换色，无 CSS 过渡）。
