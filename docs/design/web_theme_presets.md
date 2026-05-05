# Web 前端：多套预设主题（扩展 `data-theme`）设计

**状态**：设计稿（**未**承诺实现时间表）。**受众**：维护 `frontend/` 样式与设置相关 Rust 的开发者。  
**语言**：中文。  
**关联**：

- 现状与 token：**`frontend/styles/tokens.css`**
- 主题同步 DOM / 存储：**`frontend/src/app/app_shell_effects/sync_dom.rs`**（`wire_sync_theme_to_storage_and_dom`）
- 偏好键：**`frontend/src/app_prefs.rs`**（`THEME_KEY` = `crabmate-theme`）
- 设置 UI：**`frontend/src/app/settings_sections.rs`**、**`settings_modal.rs`**、**`settings_page.rs`**
- 初始化：**`frontend/src/app/app_signals.rs`**
- 界面美化总览：**`docs/Web界面美化设计.md`**（若与本设计交叉，以本设计为准定义「多预设」契约，实现后可在该文增加指针）

---

## 1. 背景与现状

### 1.1 当前机制

| 层级 | 行为 |
|------|------|
| **持久化** | `localStorage["crabmate-theme"]` 存字符串，默认读不到时用 **`light`**（见 `app_signals.rs`）。 |
| **DOM** | `<html data-theme="...">` 由 `wire_sync_theme_to_storage_and_dom` 写入，与 `RwSignal<String>` 同步。 |
| **色板** | **`tokens.css`** 中 `:root` 为默认深色语义；**`:root[data-theme="light"]`** 覆盖浅色变量（`--bg`、`--surface`、`--accent` 等）。 |
| **局部补丁** | 多个样式文件中存在 **`:root[data-theme="light"]`** 下的组件级规则（如 `base.css`、`shell-ds.css`、`layout-chat.css`、`components.css`、`modal.css`、`status.css`），用于浅色下的细调。 |
| **设置 UI** | 设置弹窗/设置页中两个选项：**`dark`** / **`light`**，绑定 `appearance_theme`。 |

### 1.2 问题

- 仅二元主题时，**「预设风格」**（如暖色浅色、冷色深色、高对比）无法表达。
- 若直接增加 `data-theme="light-warm"` 等取值，但未处理 **仅针对 `light` 写死的选择器**，会出现 **token 已换、局部样式仍按 `light` 分叉** 的视觉错位。

---

## 2. 设计目标

1. **多套预设**：用户可选 **N 套**离散主题（每套有稳定 **slug**，如 `dark`、`light`、`dark-ocean`），仍用 **单一 `data-theme`**，不引入第二套并行主题系统（除非未来刻意拆分「明暗 + 强调色」维度，见 §6）。
2. **实现成本可控**：优先 **扩展 CSS 变量表**；Rust 侧以 **字符串 slug + 白名单校验** 为主，避免大改 `App` 状态结构。
3. **与现有偏好正交**：**`data-bg-decor` / `crabmate-bg-decor`**、**`prefers-reduced-motion`**（`motion.css`）保持不变。
4. **可维护**：合法 slug 列表在 **一处权威**（建议 Rust 常量 + 文档同步），防止 UI 选项与 CSS 块不同步。

---

## 3. 推荐方案：扩展 `data-theme` 取值

### 3.1 契约

- **`data-theme`** 的取值为 **小写 kebab-case slug**，与 `localStorage` 存值一致。
- **必选基线**：至少保留 **`dark`**、**`light`**（可与当前默认对齐）；新增预设均为额外 slug。
- **未知值**：启动或读存储后若 slug 不在白名单，**回退**到 `light` 或 `dark`（产品决定默认回退项，建议与当前 `app_signals` 默认一致并写清）。

### 3.2 CSS 结构

对每个 slug 增加一块 **完整变量覆盖**（与现有 `light` 块同级），例如：

```css
:root[data-theme="dark-ocean"] {
  --bg: ...;
  --surface: ...;
  --accent: ...;
  /* 其余与 tokens.css 默认集合同名变量一并赋值，避免半套主题 */
}
```

**可选整理**：将默认 `:root` 改为显式 `:root[data-theme="dark"]`，使「无属性」与「暗色」不重复；若保留无属性默认，须在文档中说明优先级。

### 3.3 浅色相关「组件分叉」→ token 覆层（已完成收敛）

历史上以下文件含 **`:root[data-theme="light"]`** 的额外规则；现已 **迁入 `tokens.css` 的覆层变量**（见 §3.2 注释与 `:root` / `:root[data-theme="light"]` 内 `--selection-bg`、`--page-bg-decor-*`、`--nav-rail-bg`、`--btn-primary-*`、`--modal-backdrop-bg`、`--status-agent-select-bg-image`、`--composer-bar-box-shadow`、`--composer-ws-ref-*`），**`base.css`、`shell-ds.css`、`layout-chat.css`、`components.css`、`modal.css`、`status.css`** 仅引用 `var(--*)`，**不再**按 `light` 写死选择器。

**推荐路径（优先）**：

- **A. 变量化**（现状）：新增浅色预设时 **复制 `:root[data-theme="light"]` 整块** 为新 slug（如 `light-warm`），覆写 **色板变量 + 上列覆层变量** 即可；多预设时可将浅色共用覆层提取为 **`@layer` + 共用选择器列表** 或 **`themes/*.css`**，见 §3.4。

**备选路径**：

- **B. 选择器分组**：约定浅色系 slug 前缀 `light-*`，使用  
  `:root[data-theme="light"], :root[data-theme^="light-"] { ... }`  
  （注意：`[data-theme^="light-"]` **不会**匹配 `light` 本身，须与 `light` 并列写。）
- **C. 每预设复制选择器**：维护成本高，仅作权宜。

### 3.4 文件组织（可选）

- 预设较多时，可将各主题块拆到 **`frontend/styles/themes/*.css`**，在 **`index.html`** 的 `data-trunk` 链中 **置于 `tokens.css` 之后**，便于覆盖默认 token；须在 **`docs/开发文档.md`** 的样式链说明中同步路径。

---

## 4. Rust / Leptos 改动要点

| 区域 | 改动 |
|------|------|
| **`app_prefs.rs`** | 保留 `THEME_KEY`；可增加 **`pub const THEME_PRESETS: &[ThemePreset]`** 或 `&[&str]` + 显示名 i18n key（单一事实来源）。 |
| **`app_signals.rs`** | 读 `localStorage` 后 **`normalize_theme_slug(s)`**：非白名单 → 默认。 |
| **`settings_sections.rs`** | 用预设列表驱动 UI（按钮组 / 下拉 / 网格），替代硬编码两个 `on:click`。 |
| **`settings_modal.rs` / `settings_page.rs`** | 与设置区块 **同一套预设来源**；`apply_theme_preview_to_dom` 已通用，仅需传入合法 slug。 |
| **`settings_form_state.rs` / `settings_commit.rs`** | 仍为 `String` 即可；提交时写入 `theme` signal。 |
| **i18n** | `frontend/src/i18n/settings.rs`（或等价模块）：每个 slug 的 **用户可见名称**、设置页说明一句。 |

**测试（建议）**：对 `normalize_theme_slug` 做 `#[cfg(test)]` 单元测试（未知值、空串、旧值兼容）。

---

## 5. 无障碍与产品可选能力

- **高对比预设**：建议单独 slug（如 `high-contrast` 或 `light-high-contrast` + `dark-high-contrast`），满足「不仅是换色相」的合规叙事；与 **`a11y.rs`** 关注点可交叉引用，但 **不替代** 系统 **`prefers-contrast`** 媒体查询（若未来支持自动跟系统，另文约定优先级：用户显式选主题 > 系统偏好）。
- **焦点环**：每套预设检查 **`--focus-ring-*`**（若已用变量）在 `surface` / `bg` 上的可见性。
- **语义色**：`--error` / `--success` / `--warn` 在每套预设上保持可读；避免仅调 `--accent` 导致状态色与环境糊成一团。

---

## 6. 可选演进：明暗与强调色二维拆分

若产品希望「同一浅色下多种品牌色」且 **减少** 浅色×强调色的笛卡尔积：

- 保留 **`data-theme`** 仅表示 **亮/暗档位**（或背景层级）。
- 新增 **`data-accent="blue" | "green" | ..."`**（或写入 `style` 的 `--accent`），在 CSS 中用  
  `:root[data-theme="light"][data-accent="green"] { ... }`  
  组合覆盖。

本设计稿 **默认** 采用 **§3 单属性多 slug** 以降低 DOM 与存储复杂度；若采用二维模型，须同步修改 `wire_*`、localStorage 键策略及 README。

---

## 7. 文档与回归

| 项 | 动作 |
|----|------|
| **`frontend/README.md`** | 更新 `crabmate-theme` 合法取值说明。 |
| **`docs/开发文档.md`** | 「样式结构」小节增加指向本文的链接。 |
| **`docs/frontend/VISUAL_REGRESSION_CHECKLIST.md`** | 为每个新预设增加一条手测（聊天列、模态、侧栏、composer、状态栏）。 |

---

## 8. 非目标（当前共识）

- **用户任意自选十六进制色盘**（需对比度守卫与语义色独立策略，另立项）。
- **按会话/服务端下发主题**（仍属本机 `localStorage` 偏好；若未来账号系统统一主题，再扩展同步层）。

---

## 9. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-05-01 | 初稿：扩展 `data-theme`、CSS 与分叉选择器处理、Rust 白名单与设置 UI、无障碍与可选二维 accent、文档与回归清单。 |
| 2026-05-01 | 浅色相关组件分叉迁入 `tokens.css` 覆层变量（`--selection-bg`、`--page-bg-decor-*` 等），业务 CSS 仅引用变量。 |
