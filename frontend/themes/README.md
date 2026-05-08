# Web 预设主题（`data-theme`）

本目录存放 **按 slug 拆分** 的 CSS 覆层：每条 `[data-theme="…"]` 对应设置里可选的一套外观（与 `localStorage["crabmate-theme"]` 一致）。

## 加载顺序

Trunk 在 **`frontend/index.html`** 中于 **`styles/tokens.css` 之后** 链接本目录下的文件。  
`tokens.css` 提供间距、字号、动效时长及 **默认深色色板**（`:root`，等价于壳层选用 **`dark`** 时的观感）。

## 维护清单（新增或重命名主题时）

1. **CSS**：在本目录新增 `your-slug.css`，内含完整的 `:root[data-theme="your-slug"] { … }` 色板与覆层变量（可复制 `light.css` 再改值）。
2. **`index.html`**：追加一行 `<link data-trunk rel="css" href="themes/your-slug.css" />`（须在 `tokens.css` 之后）。
3. **Rust 白名单**：`frontend/src/app_prefs.rs` 中 **`THEME_SLUGS`** 加入 slug；未知存储值会回退为 `light`。
4. **文案**：`frontend/src/i18n/settings.rs` 中 **`settings_theme_preset_label`**（或等价函数）增加显示名。
5. **文档**：`docs/design/web_theme_presets.md` 可作设计延伸参考。

## 本地自定义（不提交仓库）

可复制 **`custom.example.css`** 为 `custom.css`，按需修改并在 **`index.html`** 引用（注意 `.gitignore` 是否忽略 `custom.css`，避免误提交）。

## 内置文件

| 文件 | `data-theme` | 说明 |
|------|----------------|------|
| `light.css` | `light` | 浅色纸灰 + 钢蓝点缀 |
| `material.css` | `material` | Material 圆角 + 中性灰深色 |

**`dark`** 仍由根目录 **`styles/tokens.css`** 中 `:root` 提供，无需单独文件；若希望「深色也单独成文件」便于 fork，可从 `:root` 复制变量到 `dark.css` 并改用 `:root[data-theme="dark"]`（须与 `tokens.css` 避免重复定义）。
