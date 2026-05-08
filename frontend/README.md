# CrabMate Web（Leptos + WASM，实验性）

当前项目唯一 Web 前端实现；`cargo run -- serve` 读取 **`frontend/dist`**。

## 依赖

- Rust **`wasm32-unknown-unknown`**：`rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev/)：`cargo install trunk`（本仓库 CI/开发机已用 0.21.x）

构建时若环境变量 **`NO_COLOR=1`**，部分 Trunk 版本会报错，可先 `unset NO_COLOR` 再执行 `trunk build`。

样式源文件按模块拆在 **`frontend/styles/*.css`**，离散 **`data-theme`** 预设另放在 **`frontend/themes/*.css`**（见 **`themes/README.md`**）；**Trunk** 在 **`index.html`** 里对每条 CSS 使用 `<link data-trunk rel="css" href="…">` 打入 `dist`（单文件 `@import` 在打包后路径会失效，勿只靠根目录 `styles.css` 聚合）。修改后仍需 `trunk build` 生成 `dist`。发版前视觉手测清单见 **`docs/frontend/VISUAL_REGRESSION_CHECKLIST.md`**。

## 构建

- **日常调试**：`trunk build`（Trunk 在 dev 模式下不跑 `wasm-opt`，构建更快）。
- **发布 / 与生产体积一致**：`trunk build --release`（启用默认 `wasm-opt`，WASM 更小、冷启动通常更好）。

```bash
cd frontend
trunk build          # 开发
# 或
trunk build --release
```

产物在 **`frontend/dist/`**。

`index.html` 中 **`rel="rust"`** 未设置 **`data-wasm-opt`** 时即按上述规则区分；若要在 release 构建中也跳过优化，可给该标签加 **`data-wasm-opt="0"`**；更激进压体积可用 **`data-wasm-opt="z"`** 等（见 [Trunk](https://trunkrs.dev/) 文档）。

## 能力与现状

已覆盖当前 Web 端能力：顶栏、聊天列表 + 输入框、`POST /chat/stream` SSE、命令审批条、工作区列表、任务清单勾选、本地会话列表（弹窗内可重命名、删除、下载 **JSON** / **Markdown** 导出，JSON 为 `ChatSessionFile` v1，与 CLI `save-session` 落盘同形）。助手流式阶段回答区 DOM 按最小间隔（约 72ms）与 **rAF** 合并刷新；结束流式后在同一段逻辑内同步写入终态 HTML，并用世代门禁避免尾随定时器盖住 Markdown。助手消息处于错误态（`state: error`）时气泡内提供 **「重试」**：去掉该条及之后消息并以同一条用户提问重新走流式对话。右列工具栏「**设置**」弹窗内可切换主题预设 **`dark` / `light` / `material` / `high-contrast`**（`localStorage`：`crabmate-theme`，合法取值见 **`src/app_prefs.rs`** 中 **`THEME_SLUGS`**）与是否显示页面背景光晕（`crabmate-bg-decor`）。自定义主题：复制 **`themes/custom.example.css`** 并按 **`themes/README.md`** 接入。状态栏在 `GET /status` 失败时展示错误说明与「重试」按钮。动效：生成中/工具执行中状态点脉冲、右列宽度与显隐过渡、窄屏左栏抽屉滑动、工作区/任务列表加载后行级渐显；`prefers-reduced-motion: reduce` 下在 `styles/motion.css` 统一关闭相关动画与尺寸/transform 过渡，并显式落到终态以避免布局闪跳（主题切换仅变量换色，无 CSS 过渡）。
