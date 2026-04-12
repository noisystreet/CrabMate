# CrabMate Web 前端目标架构（Leptos / WASM）

本文描述 **`frontend-leptos/`** 期望演进的**页面与模块架构**，用于指导后续重构；**当前代码未必已完全实现**下文目标形态，以 Git 历史与 [`docs/DEVELOPMENT.md`](../DEVELOPMENT.md) 模块索引为准。

## 1. 文档目的

- **统一语言**：命名「域 / 功能 / 接线 / 端口」时的约定，减少 `App` 与子模块之间的随意耦合。
- **约束依赖方向**：视图 → 功能状态 → 共享领域逻辑 → `api` / 浏览器 API，避免反向依赖与循环模块。
- **可渐进迁移**：允许与现有 `src/*.rs`、`app/*.rs` 并存，按阶段搬迁而非一次性重写。

## 2. 技术栈约束（不可回避）

- **Leptos CSR**：入口为单根 [`App`](../../frontend-leptos/src/app/mod.rs)，**细粒度响应式**（`RwSignal` / `Effect`）是状态主模型。
- **WASM**：无传统「多线程共享可变状态」；跨异步边界用 `spawn_local`、共享句柄多为 `Rc` / `Arc` + 内部可变性。
- **与后端契约**：HTTP / SSE 形状以 Rust 后端与 [`docs/SSE_PROTOCOL.md`](../SSE_PROTOCOL.md) 为权威；前端**不**私自发明事件名或字段语义（见 §8）。

## 3. 设计原则

| 原则 | 说明 |
|------|------|
| **按功能域切分** | 聊天、工作区、任务、设置、模态等尽量各自成簇；避免「所有 `RwSignal` 堆在 `App`」长期化。 |
| **显式接线** | 与时间、订阅、浏览器相关的副作用集中在 **`wire_*` 函数**或小型 `Effect` 模块，不在任意组件深处散落 `spawn_local`。 |
| **状态聚合** | 同一业务横切面（如会话 + 流式 + 水合）用**单一聚合类型**持有相关 `RwSignal` 句柄（如已有 **`ChatSessionSignals`**），减少参数列表爆炸。 |
| **纯逻辑外提** | 与 DOM 无关的格式化、扫描、防抖判定、消息展示过滤等留在 `message_format`、`timeline_scan`、`debounce_schedule` 等模块，**`app/` 以组合与接线为主**。 |
| **三端逻辑复用意识** | 会话存储、导出形状、SSE 语义以后端与 `runtime` 为准；前端仅负责展示与本地 `localStorage` 持久化，**不**复制一套互不等价的业务规则（见仓库 `cli-tui-web-shared-logic` 类规则）。 |

## 4. 目标分层（逻辑依赖自上而下）

```
┌─────────────────────────────────────────────────────────┐
│  视图层（app/ 组件、view!、样式类名）                      │  仅组合 UI、事件绑定、读 signal
├─────────────────────────────────────────────────────────┤
│  功能接线层（wire_*、session_hydrate、chat_scroll 等）   │  Effect、订阅、与浏览器 API 交互
├─────────────────────────────────────────────────────────┤
│  功能状态 / 聚合句柄（*Signals、侧栏/模态开关等）          │  可随域迁移到子模块；App 只保留「壳」与全局壳级状态
├─────────────────────────────────────────────────────────┤
│  领域与工具（session_ops、session_sync、storage、i18n）  │  尽量不依赖具体组件路径
├─────────────────────────────────────────────────────────┤
│  端口：api、sse_dispatch、conversation_hydrate           │  HTTP/SSE 与后端对齐；可 mock 便于未来单测
└─────────────────────────────────────────────────────────┘
```

**依赖规则**：

- `app/*` 可以依赖 `session_*`、`api`、`storage` 等。
- `session_ops` / `storage` **不得**依赖 `app/`。
- `api` 仅依赖类型、`serde`、浏览器 fetch，不依赖具体视图。

## 5. 目标目录与模块边界（演进方向）

以下为**目标形态示意**，与当前文件一一对应关系可在重构中逐步调整命名，不必机械照搬路径。

| 区域 | 职责 | 典型内容（现有或目标） |
|------|------|------------------------|
| **`app/`** | 应用壳：布局、路由级组合、全局快捷键、将子视图串起 | `mod.rs`、**`app_shell_effects`**（壳级 `wire_*` / `Escape`）、侧栏/主列/状态栏视图 |
| **`app/chat/`** | 聊天主路径：列表、输入、滚动、查找、流式接线 | **`column`** / **`composer`** / **`composer_stream`** / **`handles`** / **`scroll`** / **`find`** / **`find_bar`** / **`message_chunks`** / **`message_row`** / **`message_group_views`** / **`timeline`**（见 **`app/chat/mod.rs`** 再导出）；**`ChatSessionSignals`** 定义在 **`src/chat_session_state.rs`**（crate 根），供 **`app/`** 与 **`session_modal_row`** 共用；流式与壳层共享的 **`ComposerStreamShell`**（**`handles.rs`**）聚合 `status_*` / 审批 / 中止 / 工作区刷新 / 变更集 / 澄清等句柄，供 **`WireComposerStreamsArgs`**、**`composer_stream::ChatStreamCallbackCtx`** 与 **`ChatColumnShell`**（主列视图）复用，避免 `App` 向流式与主列重复传入同一组 `RwSignal` |
| **`app/session_*` / 会话** | 会话列表 UI、列表模态、会话级水合 | `sidebar_nav`、`session_list_modal`、`session_hydrate` |
| **`app/workspace_*`** | 工作区树与刷新 | **`workspace_panel_state`**（**`WorkspacePanelSignals`**）+ **`workspace_panel`**（**`make_refresh_workspace`** / **`make_insert_workspace_path_into_composer`**）；与根目录 **`workspace_shell`** 协同 |
| **`app/status_tasks_*`** | `/status` 与侧栏任务 | **`status_tasks_state`**（**`StatusTasksSignals`**）+ **`status_tasks_wiring`**（拉取闭包与侧栏可见 **`Effect`**） |
| **`app/*_modal`** | 模态与独立焦点陷阱 | `settings_modal`、`changelist_modal` 等 |
| **根模块** | 跨功能复用、无 UI | `api`、`storage`、`session_sync`、`sse_dispatch`、`i18n` |

**可选的下一步演进**（非必须一次完成）：

- 将 **`chat`** 相关 `wire_*` 与聚合信号进一步收到 **`app/chat/`** 子目录（或 `features/chat` 命名空间），使 `app/mod.rs` 只做「注入句柄 + 布局」。（**`app/chat/`** 已存在；后续可继续把 **`App`** 内聊天域 `RwSignal` 收到聚合类型以瘦身 `mod.rs`。）
- **任务 + `/status`**：已用 **`StatusTasksSignals`** + **`status_tasks_wiring`**；工作区侧仍为 **`WorkspacePanelSignals`**（与旧「对称聚合」目标一致，分文件承载）。

## 6. 状态与上下文策略

- **全局壳级**：主题、语言、侧栏宽度、`SidePanelView`、仅与「壳」相关的 `RwSignal` 可保留在 **`App`**。
- **会话 + 流式**：已与 **`ChatSessionSignals`** 对齐；后续新增字段（如只读展示标志）应优先加在聚合体上，而非再增加平行的 6 个参数。
- **模态与抽屉**：各自用独立 `RwSignal<bool>` 或小型结构；**关闭顺序**（Escape 层级）由 **`app_shell_effects::wire_escape_key_layered_dismiss`**（或同类集中模块）处理，避免多处重复监听。
- **Context**：若未来子树加深，可对「只读下传」的句柄使用 Leptos **Context** 减少 props 钻孔；**慎用**全局隐式 context，以免调试困难。

## 7. 副作用与 `wire_*` 约定

- **`wire_*` 函数**：负责注册 `Effect`、连接 `spawn_local`、把 **`api`** 结果写回 `RwSignal`；命名保持 **`wire_<域>_<行为>`**（如 `wire_session_hydration`）。
- **单一职责**：同一类外部事件（如水合 nonce 变化）对应**一条清晰的数据流**，避免两个 `Effect` 同时写同一消息列表而不加版本/token 协调。
- **测试**：纯函数逻辑优先放在非 `app` 模块；WASM 单测用 `wasm-bindgen-test`（见 [`docs/TESTING.md`](../TESTING.md)）。

## 8. 与后端的契约边界

- **路由与请求体**：变更须与后端 Axum handler 及 [`docs/DEVELOPMENT.md`](../DEVELOPMENT.md) 中说明一致。
- **SSE**：行协议、错误码、控制面 JSON 以 [`docs/SSE_PROTOCOL.md`](../SSE_PROTOCOL.md) 与 `crabmate-sse-protocol` 版本为准；前端解析集中在 **`sse_dispatch`**，**`app/` 只做回调挂载**。
- **新增能力**：优先在后端与协议中落地字段，再更新前端类型与 `sse_dispatch`，避免「前端先写死字符串」。

## 9. 分阶段重构路线（建议）

阶段 | 目标 | 完成判据（建议）
------|------|------------------
**A. 巩固聚合** | 新增长会话相关状态优先进入 **`ChatSessionSignals`**（或同类聚合），`App` 不再增加平行的会话 `RwSignal` | 新 PR 不扩大 `wire_chat_composer_streams` 参数列表
**B. 壳与域分离** | `app/mod.rs` 仅保留布局组合 + 全局 `Effect`，会话/工作区/任务各自的 `Effect` 块可迁到对应子模块的 `wire_*` | `mod.rs` 行数持续下降或由脚本统计不再增长
**C. 功能子目录** | `chat` 相关文件物理上归入 `app/chat/`（或等价命名），`mod` 再导出 | **已落地** `app/chat/`；`docs/DEVELOPMENT.md` 已同步 |
**D. 端口清晰** | `api.rs` 保持最薄；如需 mock，对 `fetch_*` 层包一层 trait 或测试桩（按需） | 关键 `fetch` 在 `wasm-bindgen-test` 或集成测试可替换

**注意**：每一阶段完成后应 **`cd frontend-leptos && cargo check --target wasm32-unknown-unknown`**，并与 [`docs/SSE_PROTOCOL.md`](../SSE_PROTOCOL.md) / 前端解析路径交叉检查。

## 10. 反模式（应主动纠正）

- 在 **`app/mod.rs`** 内新增大段业务逻辑（水合、重试、与工作区无关的解析）。
- 多个 `Effect` 无协调地 **`sessions.update`** 写同一会话消息列表，导致竞态与闪烁。
- 在组件中直接拼 URL / 手写与后端不一致的 JSON（应走 **`api`** 与共享类型）。
- 为省事引入新的全局 `static mut` 或跨模块可变单例（与 Leptos 响应式模型冲突且难测）。

## 11. 相关文档

- [`docs/DEVELOPMENT.md`](../DEVELOPMENT.md)：`frontend-leptos` 模块索引与维护约定。
- [`docs/SSE_PROTOCOL.md`](../SSE_PROTOCOL.md)：流式协议。
- [`docs/TESTING.md`](../TESTING.md)：前端构建与测试命令。
- [`frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md`](VISUAL_REGRESSION_CHECKLIST.md)：视觉回归自检（若有 UI 大改）。

---

**修订策略**：当目标目录结构或分层原则发生实质变化时，更新本文并同步 [`docs/DEVELOPMENT.md`](../DEVELOPMENT.md) 中 `frontend-leptos` 小节或 [`README.md`](../README.md) 文档表（若需对外可见索引）。
