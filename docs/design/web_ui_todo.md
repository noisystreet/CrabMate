# Web UI 未来功能规划

## 1. 设置页面（路由化）

### 1.1 背景与目标

当前设置入口为工具栏图标，点击后在主对话区**覆盖层模态框**展示。模态框会遮挡对话内容，用户无法参照对话上下文修改配置。

目标：将设置从覆盖层模态框改为独立的**全屏路由页面** `#/settings`，与对话页面分离，提供更沉浸的配置体验。

### 1.2 路由方案

- 路由格式：`/#/settings`（hash-based，不依赖第三方路由库）
- 页面列表：
  - `Chat` — `/#/` 或空 hash，默认
  - `Settings` — `/#/settings`

### 1.3 实现要点

#### 路由监听

```rust
use leptos_dom::helpers::window_event_listener;

let route = RwSignal::new(parse_hash_route());
let _hashchange_handle = window_event_listener(leptos::ev::hashchange, {
    let route = route.clone();
    move |_ev: web_sys::HashChangeEvent| {
        route.set(parse_hash_route());
    }
});
```

#### AppShellCtx 与 view! 闭包约束

`AppShellCtx` 包含 `Rc<dyn Fn()>`、`Rc<RefCell<...>>` 等非 `Send+Sync` 类型，**不能**直接作为 `view!` 宏闭包的捕获变量。

**推荐方案**：将路由信号 `route` 作为 `App()` 顶层变量独立管理，通过 `Show when=move || route.get() == Route::Settings` 控制渲染，**不**将 `app_ctx` 捕获进该闭包。

#### CSS 布局

```css
.settings-page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
}
```

### 1.4 待解决的技术风险

- **AppShellCtx 的 Rc 闭包约束**：需确保路由 `Show` 组件不捕获 app_ctx，或将 AppShellCtx 拆分为含 Rc 和不含 Rc 两部分。

### 1.5 文件清单

| 文件 | 操作 |
|------|------|
| `frontend-leptos/src/app/mod.rs` | 添加 Route 枚举、hashchange 监听、`Show` 路由渲染 |
| `frontend-leptos/src/app/settings_page.rs` | 新建，SettingsPage 组件 |
| `frontend-leptos/src/app/side_column.rs` | 设置按钮改为 hash 跳转 |
| `frontend-leptos/src/i18n/settings.rs` | 添加 `settings_back`、`settings_back_aria` |
| `frontend-leptos/styles/modal.css` | 添加 `.settings-page` 等布局样式 |

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

- **跟随系统**：自动检测 OS 暗/亮偏好
- **主题自定义**：提供配色面板，让用户覆盖 CSS 变量（品牌色、强调色）
- **背景装饰**：当前有 bg_decor 开关，可扩展为更多背景样式（粒子、渐变图案）

## 10. 移动端适配

当前主面向桌面端，移动端体验有提升空间。

- 侧边栏默认收起，顶部汉堡菜单触发
- 触摸友好的按钮尺寸和间距
- 长消息默认折叠，减少滚动
- 审批弹窗在移动端更宽大

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

