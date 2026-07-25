# Web 聊天：从终端流向 OpenCode / OpenClaw 式流式跟底演进

**状态**：路线图 / 设计备忘（**未**承诺实现顺序与时间表）。  
**受众**：维护 **`frontend/src/app/chat/`**、滚动跟底、流式展示与相关 E2E 的开发者。  
**语言**：中文。  
**关联**：

- 模块索引：**`docs/开发文档.md`**（`column` / `tui_stream_view` / `tui_line_markdown` / `scroll_*` / `composer_stream`）
- 前端架构：**`docs/frontend/ARCHITECTURE.md`**
- SSE 契约：**`docs/SSE协议.md`**（展示层演进**不**改协议，除非另开 ADR）
- 既有气泡路径（保留未挂载）：`message_row/`、`assistant_body/`、`messages_list.rs`

---

## 1. 目标与非目标

### 目标

对标 OpenCode / OpenClaw 一类产品在聊天主路径上的体验：

1. **流式可见**：token / chunk 到达后尽快出现在视口末端。  
2. **跟底稳**：内容变高时贴底；用户上滚后不被强拉回。  
3. **布局可预期**：流式期少闪烁、少整树重排。  
4. **能力不丢**：复制、重试/再生、工具过程可见、查找/导出语义与会话数据一致。

### 非目标（本路线图不要求）

- 复刻对方 UI 视觉或品牌。  
- 引入 React/Vue 流式 Markdown 库作为默认运行时依赖（Leptos/WASM 栈优先自研或薄封装）。  
- 用展示层演进绑架 SSE / `StoredMessage` / turn-layout v2 协议破坏性变更。  
- 立刻删除气泡 / Markdown / 工具卡全部代码（可阶段性「默认路径不用」，再决定删除）。

---

## 2. 对标模式（OpenCode / OpenClaw 类）在做什么

下列为业界常见实现共性，**非**对其源码的逐行断言：

| 共性 | 含义 | 对跟底/流式的作用 |
|------|------|-------------------|
| **Append-only 主轴** | 主 transcript 以「末端增长」为主，少对历史块整体 `innerHTML` 重建 | 高度单调，滚动目标稳定 |
| **流式期轻渲染** | 生成中优先纯文本 / 按块 / 按行；重格式在闭合或回合结束 | 避免半截语法抖动 |
| **简单跟底状态机** | `pinnedToBottom` + 内容变高则 `scrollTop = max`；上滚清 pin；贴底再 pin | 规则少、竞态少 |
| **工具是事件行，不是重卡片** | 工具开始/结束以一行摘要挂在 transcript 末尾 | 少异步撑开、少折叠状态 |
| **操作栏外置或悬停轻量** | 复制/重试不依赖整套气泡 DOM | 展示层可保持扁平 |

对照：CrabMate **旧默认**（气泡 + 工具卡 + 流式全量/近全量 Markdown）把「变高」拆成多次异步布局；**当前默认**（终端流 + 按行 Markdown）已靠近上表前三项。

---

## 3. 当前基线（2026-07）

| 层级 | 现状 |
|------|------|
| **默认视图** | `chat_column_view` → `ChatTuiStreamView`（无切换按钮） |
| **数据** | 仍用 `ChatSessionSignals` + `stream_text_overlay`；发送/SSE 未分叉 |
| **渲染** | `tui_line_markdown`：闭合行 / 落定后 `to_safe_html`；半行与未闭合围栏纯文本 |
| **DOM 写入** | transcript 容器 `set_inner_html` 全量替换（按会话重建 HTML 字符串） |
| **跟底** | 共用 `ChatMessagesScrollShell` + ResizeObserver / sentinel / pointer 意图 |
| **缺口** | 无一键复制/重试 UI；工具过程在 transcript 中偏扁；全量 HTML 重建在超长会话会变贵；气泡路径 `allow(dead_code)` 保留 |

---

## 4. 目标架构（演进终点草图）

```text
SSE / overlay / sessions（不变）
        │
        ▼
┌───────────────────────────────┐
│ TranscriptModel（只读投影）    │  稳定 turn/block id + 末尾「活块」
└───────────────────────────────┘
        │
        ├─ 历史块：已提交 DOM（不再每 token 重写）
        └─ 活块：仅追加 / 替换尾段（纯文本或按行 HTML）
        │
        ▼
┌───────────────────────────────┐
│ StickToBottomController       │  pin + ResizeObserver(内容根)
└───────────────────────────────┘
        │
        ▼
  Composer / 轻量操作条（复制全文、重试上一条…）
```

**不变量**：

1. 会话真源仍是 `sessions` + overlay；展示层只做投影。  
2. 历史块 DOM **身份稳定**（按 message id / turn id）；只有活块可变。  
3. 跟底只观察「内容根」尺寸与用户 pin，不依赖气泡哨兵语义。

---

## 5. 分阶段计划

### Phase 0 — 固化终端流基线（已完成 / 收尾）

- [x] 默认终端流、去掉模式切换  
- [x] 按行轻量 Markdown  
- [x] 滚动 E2E + 终端流 E2E  
- [ ] 文档与待办交叉引用本文件（本条）  
- [ ] 明确「气泡路径」保留策略：默认不挂载，删除门槛另议

**验收**：冷启动即终端流；长流式跟底 E2E 绿；按行粗体流式中不闪、落定后生效。

---

### Phase 1 — 跟底状态机收敛（对标「简单 pin」）

**动机**：现有 scroll 壳来自气泡时代，规则偏多；终端流只需「贴底 pin」。

建议：

1. 抽出 `StickToBottom`（或收窄 `scroll_shell` / `scroll_follow`）专用 API：  
   - `pin` / `unpin`  
   - `on_content_resize_if_pinned`  
   - `engage_on_user_send`  
2. 终端流内容根固定为 `.chat-tui-transcript`（或外层 `.chat-thread`），ResizeObserver 只盯这一处。  
3. 用户上滚 / 拖滚动条：unpin；滚回底部阈值内：re-pin。  
4. 减少与 IntersectionObserver sentinel 的双重真相（可保留 sentinel 仅作「是否在底部」辅助，或逐步淘汰）。

**验收**：

- E2E：长流式贴底、上滚不拉回、回底恢复、生成后延迟增高仍贴底（沿用现有 scroll specs）。  
- 代码：跟底路径可在注释中画清状态图（Pinned / Unpinned）。

**风险**：改动 scroll 影响查找跳转；`focus_message_id_after_nav` 须显式 unpin 或「临时滚到目标」。

---

### Phase 2 — Append-only DOM（去掉每 token 全量 `innerHTML`）

**动机**：对标 append-only；超长会话下全量重建是下一瓶颈，也会放大跟底抖动窗口。

建议：

1. Transcript 拆成：  
   - **committed**：历史 turn 的 DOM 节点（按 `message.id` 挂载一次）  
   - **live**：当前 loading 助手尾块（只更新这一节点）  
2. overlay revision 变化时：**仅**更新 live 节点文本/HTML，不重刷 committed。  
3. 回合 `on_done` / 切换会话：把 live 固化进 committed，或整段重建一次（会话切换可全量，流式热路径不可）。  
4. 按行 Markdown：对 live 块继续「半行纯文本 + 闭合行升级」；升级时尽量只改 live 子树。

**验收**：

- 单元：live 更新不销毁 committed 节点（可用测试 id / 计数钩子）。  
- E2E：长内容流式过程中 `scrollHeight` 单调增、无整页闪白。  
- 性能备忘：人为 2万字流式时主线程长任务可接受（人工或可选 bench）。

**风险**：查找高亮、导出预览若依赖整棵 HTML，需改读模型而非 DOM。

---

### Phase 3 — 轻量操作与工具过程行

**动机**：OpenCode 类产品不靠气泡也能复制/重试；工具可见但不「重卡」。

建议：

1. **操作条**（composer 上方或 transcript 底）：  
   - 复制全文 / 复制最后一条助手  
   - 重试最后一条失败助手 / 从最后一条用户再生（复用 `message_row_actions` / `ComposerStreamFollowUp` 逻辑，不复用气泡 DOM）  
2. **工具过程**：SSE `tool_*` 在终端流中追加一行摘要（可复用 `tool_card_compact_text` 或 TUI `format_sse_payload_one_line` 思路），默认折叠详情为可展开 `<details>` 或点击复制。  
3. **不做**：流式期完整工具卡网格、多级折叠组（除非用户显式打开调试台）。

**验收**：E2E 覆盖复制按钮与一次 mock 重试；工具回合 transcript 可见工具名且跟底仍稳。

---

### Phase 4 — 流式 Markdown 质量（可选增强）

在 Phase 2 稳定后，再考虑：

| 选项 | 说明 | 建议 |
|------|------|------|
| A. 保持按行 | 现状增强（列表续行、引用） | 默认主路径 |
| B. 块级状态机 | 仅「活跃块」重渲，闭合块冻结 | 中期；对齐 optimark/stream-md 思想，Rust 自研 |
| C. JS 库 interop | generative-dom 等 | 仅实验分支；注意 CSP、包体、Leptos 边界 |

**原则**：流式期允许「好看一点」；**禁止**为完整 GFM 牺牲跟底与半截围栏稳定性。

---

### Phase 5 — 清理与双路径决策

1. 评估气泡路径：无产品需求则删除或移入 `examples/` / feature flag；有需求则「高级视图」二次挂载且**不得**夺回默认。  
2. 删除仅服务气泡的死信号/CSS；保留 `message_format` / overlay 等共享层。  
3. 更新 **`docs/frontend/ARCHITECTURE.md`** 数据流图。

---

## 6. 建议实施顺序（依赖）

```text
Phase 0（基线）
    → Phase 1（跟底 pin 收敛）     // 收益大、面可控
    → Phase 2（append-only DOM）   // 性能与稳跟底
    → Phase 3（复制/重试/工具行） // 产品完整度
    → Phase 4（Markdown 增强）    // 可选
    → Phase 5（清理）
```

并行注意：Phase 3 的「复用 follow_up」可与 Phase 1 部分并行，但 **DOM 操作条应落在 Phase 2 的 transcript 结构上**，避免先做再拆。

---

## 7. 测试策略

| 层级 | 内容 |
|------|------|
| 单元 | `tui_line_markdown`；后续 live/committed 更新边界；pin 状态纯函数 |
| E2E mock | 现有 `mock-tui-stream-view`、`mock-scroll-behavior`；Phase 3 增加复制/重试 |
| E2E 真实 LLM | 可选：三轮对话贴底（已有真实滚动 spec 时可复用） |
| 回归门禁 | `frontend` wasm check / clippy；改 scroll 时必跑 scroll specs |

---

## 8. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 跟底与查找跳转冲突 | 跳转显式 unpin；跳转完成不自动 pin，除非目标已在底部 |
| 全量 HTML → 增量迁移引入双写 bug | feature 或内部开关；先 shadow 对比再切默认 |
| 工具行刷屏 | 同 tool_call_id 更新同一行；结束态替换开始态 |
| 过早删除气泡代码 | Phase 5 前只 `allow(dead_code)` / 不挂载 |
| 引入 JS Markdown 库 | Phase 4 默认不选；若试点须过 CSP 与体积评审 |

---

## 9. 成功标准（产品体感）

1. 长回复流式过程中，视口持续贴最新内容，无「生成完再跳一截」。  
2. 用户上读历史时，生成继续也不强行拉回。  
3. 流式半截 `**` / `` ``` `` 不闪成错误结构；落定后格式正确。  
4. 复制与重试无需恢复气泡即可完成主路径任务。  
5. 超长会话下流式时 UI 仍可交互（无明显整页卡顿）。

---

## 10. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-07-25 | 初稿：自当前终端流 + 按行 Markdown 向 OpenCode/OpenClaw 式 append-only + 简单 pin 演进 |
