# Web UI 未来功能规划

## 1. 设置页面（路由化）— **已落地**

全屏设置页 + hash 路由：

- **`#/settings`** / **`#/settings/<section>`**（如 `appearance`、`mcp`）；关闭回到 **`#/`**
- 兼容旧式 **`#settings/<section>`**
- 工具栏「设置」、返回键、Esc、浏览器前进/后退与 `settings_page` 信号双向同步

实现：`frontend/src/app/settings_page/hash_routing.rs`、`view.rs`、`chrome.rs`、`side_column_toolbar.rs`。

---

## 2. 消息编辑与重发

当前用户消息发送后无法编辑重发。

- 在用户消息上显示「编辑」按钮（hover 时出现）
- 点击后进入编辑态，输入框内容替换为该消息，可修改后重新发送
- 重发时在语义上等同于在原始位置插入一条新对话（保留原消息作为历史）

## 4. 对话分支（Conversation Fork）

在任意一条助手消息处「从此处继续」创建分支，生成一条独立会话线。

- 侧边栏每个会话节点显示分支图标
- 支持在不同分支间切换
- 分支命名可自动用首条消息前 N 字

## 6. 消息反应（Reactions）

对助手消息添加简单的情绪反应（如 👍 👎 💡 ❓），用于快速反馈。

- hover 消息气泡显示反应工具栏
- 汇总显示反应统计，不占用对话空间

## 7. 流式输出状态优化

- 显示当前正在生成的 token 数量或估算时间
- 「停止生成」按钮更醒目（尤其在长输出时）
- 流式块在未完成时用虚线框标识，完成后变为实线

## 8. 代码块增强

- 一键复制代码块按钮
- 语法高亮主题支持（跟随暗/亮主题切换）
- 大代码块默认折叠，点击展开

## 9. 主题与视觉增强

- **跟随系统**：设置「配色方案」可选 **跟随系统**（`prefs.theme = system`）；`<html data-theme>` 解析为 OS / WebView 的 `prefers-color-scheme`（**`dark` / `light`**），并监听切换（设置 UI 打开时不重刷，以免冲掉外观预览）。**桌面 Linux**：主窗按 **`gsettings` `color-scheme`** 显式 `Theme::Dark`/`Light`（GNOME `prefer-dark` + `Adwaita` 时 WebKit `matchMedia` 会误报浅色；`theme(None)` 不够）；前端可走 **`os_prefers_dark_theme`**。`material` / `high-contrast` 仍为手动预设。
- **主题自定义**：提供配色面板，让用户覆盖 CSS 变量（品牌色、强调色）
- **背景装饰**：当前有 bg_decor 开关，可扩展为更多背景样式（粒子、渐变图案）

## 10. 移动端适配

当前主面向桌面端；窄屏优化见 `frontend/styles/mobile.css` 与 `app_shell_effects/viewport.rs`（`data-narrow-viewport`、侧栏 Sheet、safe-area、长消息折叠）。

- [x] 侧边栏默认收起，顶部汉堡菜单触发（`≤768px` 抽屉 + 进入窄屏自动收起右侧面板）
- [x] 触摸友好的按钮尺寸和间距（`mobile.css` 44px 最小点击区）
- [x] 长消息默认折叠，减少滚动（`chat-tui-turn--long` + 操作条「展开」；折叠高度 `min(280px, 35dvh)`）
- [x] 审批弹窗在移动端更宽大（`mobile.css` 全宽底 sheet）
- [x] 会话抽屉品牌行显式「筛选」按钮（不必依赖右键打开搜索面板）
- [x] 右缘左划打开工作区半屏抽屉（与左栏互斥；工具栏含状态/设置；点遮罩或右划关闭）

## 11. 会话元数据管理

- 会话创建时间、最后活跃时间显示
- 会话置顶（pin）
- 会话备注/标签（如 `project-a`、`debug`）
- 批量删除、合并会话

## 12. Toast / 通知系统

用于后台任务（如 changelog 拉取失败、workspace 刷新错误）的非阻塞提示。

- 右上角堆叠展示，3-5 秒自动消失
- 支持成功/警告/错误三种级别
- 点击可关闭

