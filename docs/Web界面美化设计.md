# Web 界面美化设计（Leptos 前端）

本文面向**维护与迭代 CrabMate Web UI** 的同事，说明当前前端的样式架构、美化切入点，以及「Material 主题」与 Leptos 的关系。实现细节以仓库内 **`frontend/`** 为准。

---

## 1. 目标与非目标

**目标**

- 在不大改业务逻辑的前提下，通过 **CSS 设计 token** 与 **模块化样式** 持续提升可读性、层次与一致性。
- 为后续「换肤 / 浅色模式 / 更贴近某套设计规范」提供**可执行的演进路径**。

**非目标（本文不承诺）**

- 不规定具体视觉稿像素级还原；不绑定某一商业设计系统版本号。
- 不把「必须接入某第三方组件库」列为唯一正确方案。

---

## 2. 技术栈与事实前提

| 项 | 说明 |
|----|------|
| 框架 | **Leptos**（CSR）+ **WASM**，**Trunk** 打包；入口 **`frontend/index.html`**。 |
| 样式 | **手写模块化 CSS**（非 Tailwind 全站类名方案）；全局变量集中在 **`frontend/styles/tokens.css`**。 |
| 字体 | **`index.html`** 通过 Google Fonts 引入 **DM Sans**、**JetBrains Mono**（与正文/代码展示分工一致）。 |

**Leptos 是否有「官方 Material 主题」？**

- **没有。** Leptos 提供响应式 UI 与 DOM 渲染，**不附带** Material Design 成套皮肤或组件库。
- 若需要 **Material / MD3 观感**，需在工程侧自行引入：**Material Web（Web Components）**、**MDC 样式 + 自建结构**、或仅采用 **Material Symbols** 等图标资源；与 Leptos 通过普通 DOM/CSS 协作，无框架级「一键主题」。

---

## 3. 当前样式分层（维护者地图）

Trunk 要求 **`index.html`** 中 **`data-trunk` 的 `link rel="css"`** 与磁盘文件一致；**新增全局样式文件时必须在此追加一条**，否则 **`dist`** 会缺样式。

建议阅读顺序：

1. **`styles/tokens.css`** — 颜色、间距、圆角、动效时长、语义色等 **`:root` 变量**；换肤优先改这里。
2. **`styles/base.css`** — 全局 reset、根排版、链接与基础控件。
3. **`styles/components.css`** — 按钮、气泡、表单片段等复用块。
4. **`styles/layout-chat.css`** — 聊天主栏、消息流、输入区域布局。
5. **`styles/sidebar.css`** / **`styles/status.css`** / **`styles/approval.css`** / **`styles/modal.css`** — 侧栏、状态栏、审批、弹层。
6. **`styles/shell-ds.css`** — 外壳与设计系统衔接（若有布局壳层约定，以此为准）。
7. **`styles/motion.css`** — 入场、侧栏、流式首段等动效；与 token 中的 **`--dur-*`**、**`--ease-*`** 配合。

**原则**：业务组件在 Rust 里尽量只挂 **语义化 class**，具体视觉落在上述 CSS 模块中，避免在 `view!` 里堆长串内联样式。

---

## 4. 美化维度（可独立推进）

以下各条可单独开任务，合并时注意 **对比度** 与 **焦点可见性**（键盘导航、`focus-visible`）。

### 4.1 设计 Token（最高杠杆）

- **背景层级**：`--bg` / `--bg-elevated` / `--surface` 的明度差决定「浮起感」；避免过多层仅差 1～2% 的灰，导致界面发糊。
- **主色与语义色**：`--accent`、`--info` / `--success` / `--warn` / `--error` 及其 **`--*-bg`** 混色背景，用于状态条、提示条、消息内标签。
- **边框与分隔**：`--border` / `--border-subtle`；暗色主题下弱边框 + 轻阴影往往比粗线框更干净。
- **间距与圆角**：统一使用 token 中的 **`--space-*`**；圆角若尚未全面 token 化，新增时优先在 **`tokens.css`** 增补 **`--radius-*`** 再引用，避免魔法数散落。

### 4.2 排版与信息密度

- 标题/侧栏分组/消息元信息：**字重、字号阶梯、行高** 与 `--text` / `--muted` 对比一致。
- 长会话：**消息组间距**、引用块、代码块与正文的呼吸感（见 **`layout-chat.css`** 与 prose 相关规则）。

### 4.3 组件状态

- **Hover / Active / Disabled**：按钮与列表项需成套状态，避免仅 default 样式。
- **加载与空状态**：流式等待、无消息、无工作区时的占位与骨架（若有）应共用 token，避免一处一色。

### 4.4 动效

- 优先使用 **`tokens.css`** 中的时长与缓动变量，保证侧栏、消息入场、布局列宽变化 **体感一致**。
- 动效应可尊重 **`prefers-reduced-motion`**（在 **`motion.css`** 或 base 层统一降级）。

### 4.5 可访问性

- 焦点环：与 **`--focus-ring-*`** 一致，保证键盘用户可见。
- 交互目标最小点击区域、对比度（尤其 **`--muted` 小字`** 与彩色背景组合）建议对照 WCAG 2 AA 自检。

---

## 5. 演进路线选型

### 路线 A：强化现有 Token + CSS（推荐默认）

- **成本**：低～中；与当前架构一致。
- **适用**：深色微调、浅色模式、品牌色替换、密度选项。

### 路线 B：引入 Material Web / MDC 等（可选）

- **成本**：中～高；需处理 **Shadow DOM 样式穿透**、**包体与 CDN**、与现有 class 的并存策略。
- **适用**：明确要求 MD3 控件形态（FAB、SnackBar、特定 List 行为）时评估。

### 路线 C：Utility-first（如 Tailwind）

- **成本**：高（构建链、与现有 CSS 的融合、类名策略）；**非**当前仓库默认方向，若引入需单独 RFC 级讨论。

---

## 6. 实施与自检清单

改动样式或 `index.html` 后建议至少执行其一（与 **`.cursor/rules/frontend.mdc`** 一致）：

```bash
cd frontend && cargo check --target wasm32-unknown-unknown
# 发版或大范围 UI 改动前：
cd frontend && trunk build --release
```

**提交前自检（样式相关）**

- [ ] 新增/重命名 CSS 文件已写入 **`frontend/index.html`** 的 `data-trunk` 链。
- [ ] 未在源码中引入可误认的真实密钥或 token（见 **`.cursor/rules/secrets-and-logging.mdc`**）。
- [ ] 若变更 **用户可见** 行为或显著外观，按 **`docs/待办清单.md`** / **`README.md`** 约定评估是否同步文档。

---

## 7. 相关文档

- **架构与前端位置**：**`docs/开发文档.md`**（总览中的 Web 前端一节）。
- **开发与构建**：**`AGENTS.md`**、**`.cursor/rules/frontend.mdc`**。
- **SSE 与 UI 逻辑**：样式变更一般不触及 **`frontend/src/sse_dispatch.rs`**；若同时改协议消费路径，遵守 **`.cursor/rules/api-sse-chat-protocol.mdc`**。

---

## 8. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-05-01 | 初版：基于 `frontend/styles/` 与 Leptos 无内置 Material 主题的事实整理。 |
