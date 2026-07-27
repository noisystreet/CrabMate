# 终端 TUI 对齐 Tauri / Web 展示规划

**状态**：路线图（**P1–P4 与 Phase 1–5 已落地**；Phase 6 未承诺工期）。  
**受众**：维护 **`src/runtime/tui/`**、**`crates/crabmate-turn-layout`**、**`crates/crabmate-tool-card`** 与相关文档的开发者。  
**语言**：中文。  
**关联**：

| 文档 | 用途 |
|------|------|
| **`docs/Turn布局设计.md`** | Canonical Turn / `project_turn_web_v2` / Web `TurnLayout` 权威说明 |
| **`docs/design/TUI_CLI改造实施步骤.md`** | CLI→TUI 壳与对话闭环（阶段 A–D）；**布局骨架**已完成，展示对齐见本文 |
| **`docs/design/web_tui_stream_to_opencode_style.md`** | **Web** `ChatTuiStreamView` 流式跟底演进（与 ratatouille **无**代码共用） |
| **`docs/命令行与路由.md`** | `crabmate tui` 能力、导出 `projection=raw|display` |
| **`.cursor/rules/cli-tui-web-shared-logic.mdc`** | 三端共享逻辑原则 |

---

## 1. 命名澄清（必读）

| 名称 | 实际含义 |
|------|----------|
| **`crabmate tui`** | 终端全屏 UI：`src/runtime/tui/`（ratatouille） |
| **Web / Tauri「chat-tui」** | Leptos DOM 主列（`ChatTuiStreamView`）；Tauri 只是 WebView 壳，**与浏览器同一套 frontend** |
| **对齐目标** | **行序 + 文案语义** 贴近 Tauri；**不是**共用 DOM class / CSS / 像素布局 |

---

## 2. 目标与非目标

### 目标

1. **单轮工具回合**：旁白在对应工具之前、终答在工具批之后——与 Web `project_turn_web_v2` 同行序。  
2. **工具摘要**：聊天区默认 compact 与 Web/Tauri `tool-card` / 快照 `display_content` 同源。  
3. **流式不闪空、不双显**：工具相旁白由投影承接；scratch 尾挂只承担「尚未投影」的增量（如 post-tool 终答）。  
4. **可回归**：金样 / fixture 锁住「Web 投影行序 ⊆ TUI 可见文本序」。  
5. **共享停在契约层**：`Turn` + `ProjectedRow` + `tool-card`；不搬 Leptos `TurnLayout` 状态机进 ratatouille。

### 非目标

- 复刻 Web 主题、复制条、编辑器双栏、滚动 sticky 的 DOM 实现。  
- 把导出默认改成 `display`（破坏 `tool-replay` / raw 会话）。可增加**可选** display 导出，默认仍 `raw`。  
- 强制 CLI stdout（`repl` 无 SSE 回显路径）立刻接完整 turn-layout（可另开子项）。  
- 用 TUI 改造绑架 SSE 协议破坏性变更。

---

## 3. 架构（当前与目标）

```text
                    ┌─────────────────────────────────────┐
                    │  Shared: run_agent_turn · SSE · SQLite │
                    │  crabmate-turn-layout · tool-card      │
                    └───────────────┬─────────────────────┘
           ┌────────────────────────┼────────────────────────┐
           ▼                        ▼                        ▼
   终端 TUI                      Web / Tauri               CLI REPL
   runtime/tui/                  frontend/                 stdout
   turn_project.rs               TurnLayout + overlay      （投影可选后置）
   transcript.rs                 ChatTuiStreamView
```

**已落地触点（P1–P4）**

| 能力 | 路径 |
|------|------|
| SSE → TurnReducer → `project_turn_web_v2` | `src/runtime/tui/run_session/turn_project.rs` |
| 中区 `[Turn 投影]` 块 | `render.rs` / `tui_prepare_chat_body_*` |
| 工具相隐藏与投影重复的 scratch 正文 | `should_hide_streaming_content` |
| 历史/工具文案优先 tool-card | `crates/crabmate-runtime/src/message_display.rs` → `tool_content_for_display_for_message` |
| 金样 | `golden_web_v2_row_order_preserved_in_tui_projection_block`（复用 `fixtures/turn_project_golden.jsonl`） |

**仍分叉**

| 点 | 终端 TUI | Web / Tauri |
|----|----------|-------------|
| 历史回合行序 | 已定稿回合经 `CommittedTurns` flush 投影行序；会话切换仍 reseed Message[] | 全程 `StoredMessage` 投影 id |
| 终答 | 工具批结束后进投影 `[终答]`（对齐 `turn-final-answer`） | `turn-final-answer` + overlay |
| 控制面附录 | 默认仅错误/思维迹/未投影 timeline；工具事件不附录 | 事件变成独立消息行 |
| 绘制 | 整块 `Paragraph` + 按行标题着色（旁白/工具/终答） | per-section DOM + 局部 patch |
| 导出默认 | 默认仍 `projection=raw`；可选 `--projection display` / slash `display` | UI 导出多为 `display` |

---

## 4. 分阶段路线图

### Phase 0（已完成）— 本轮投影与文案同源

对应先前约定的 **P1–P4**：

| 项 | 内容 | 验收 |
|----|------|------|
| P1 | 接入 turn-layout；中区 `[Turn 投影]` | 多工具回合旁白在工具前 |
| P2 | 工具摘要走 tool-card compact | 与 `web_client_snapshot` / hydrate 金样一致方向 |
| P3 | 流式旁白不双显 | 工具相不出现「投影 + 生成中」同文 |
| P4 | 金样行序 | `cargo test --lib golden_web_v2_row_order_preserved_in_tui_projection_block` |

### Phase 1 — 历史回合并入投影语义（已落地）

**问题**：回合结束后只靠 `messages_to_transcript(Message[])` 刷新，本轮投影块清空后，历史旁白/工具序可能退回 OpenAI 落盘序。

**落地**：`transcript::CommittedTurns`；`submit_ev` 在 `finalize_for_display` 后 `flush_completed_turn` 再 `reset`；有投影时定稿为 user 前缀 → `[Turn 投影]` → 终答后缀（跳过 `tool` / 含 `tool_calls` 的 assistant）。会话切换 `msg_len` 不一致时 reseed Message[]。

**主要触点**：`submit_ev.rs`、`transcript.rs`、`mod.rs`（`TuiModel::committed_turns`）。  
**验收**：`flush_keeps_commentary_before_tool_after_projection_reset`；同一多工具回合结束后旁白仍在工具前。

### Phase 2 — 终答进入投影（已落地）

**问题**：post-tool 终答仍挂在 scratch「生成中」；与 Web `turn-final-answer` 不对齐。

**落地**：`TurnToolPhaseEnd` 后 `tool_phase_ended`；`format_projection_block(scratch)` 追加 `[终答]`；`should_hide_streaming_content` 隐藏终答副本；`finalize_for_display` 固化 `final_answer_text`；flush 时跳过 Message[] 尾部 assistant 双显。

**验收**：`post_tool_final_answer_lands_in_projection_not_streaming_tail`；工具批结束后终答在投影区工具之后。

### Phase 3 — 收敛 `[SSE 控制面]`（已落地）

**问题**：投影已含工具名/摘要时，控制面「· 工具 ·」行重复。

**落地**：`sse_mirror::format_sse_payload_one_line` 对 `ToolCall` / `ToolResult` / `ToolRunning` / `ToolOutputChunk` / `ParsingToolCalls` 及已投影的 `timeline_log` kind 返回 `None`；保留错误、思维迹与其余 timeline。

**验收**：`projected_tool_events_skip_control_plane_appendix`；常规多工具回合无「投影工具 + 控制面工具」双列。

### Phase 4 — 按行渲染（ratatouille 列表）（已落地 · 首刀）

**问题**：整串 `Paragraph` 不利于分色、按行滚动、局部刷新。

**落地**：`chat_body_to_styled_text` / `chat_line_header_style`：旁白青、工具黄、终答绿、投影头品红；`color=false` 时纯文本。滚动仍按原文估算（既有 clamp）。未改为独立 `List` widget（可后续再拆）。

**验收**：`chat_line_headers_get_distinct_styles_when_color_on`。

### Phase 5 — 导出与对照（已落地）

**落地**：`save-session --projection raw|display`（默认 raw）；`/export` `/save-session` 可选 `display`；`write_json_export_with_projection` + `messages_to_display_export`。display **不可**直接 `tool-replay`。

**验收**：`display_export_sets_projection_and_skips_system`；CLI `--help` 含 projection。

### Phase 6 — 自动化回归

**方向**：

- 复用或新增 SSE fixture（可参考 e2e `mock-export-analyze-project-flicker` 时序），经 TUI `turn_project` 断言行序。  
- 可选：`crabmate sse-replay` 输出与 TUI `format_projection_block` 文本 diff 金样。  
- 不必上 Playwright 驱动 ratatouille（成本高）；优先库测 + CLI 对照。

**验收**：CI 中有一条锁定「旁白在工具前」的 TUI 侧测试（库测即可）。

---

## 5. 明确不共享 / 后置

| 项 | 说明 |
|----|------|
| 绘制栈 | ratatouille vs Leptos DOM |
| 审批 / 澄清控件 | 协议共享，UI 分叉 |
| 无障碍 / 弱终端 | 见 **`docs/待办清单.md`** runtime 章；不阻塞 Phase 1–2 |
| 观众角色等编排能力 | 三端一起做；见 **`docs/design/audience_critic_role.md`** |
| CLI stdout 完整投影 | 另开；本文以 **`crabmate tui`** 为主 |

---

## 6. 建议落地顺序与拆 PR

| 顺序 | Phase | 建议 PR 粒度 |
|------|-------|----------------|
| 1 | ~~Phase 1 历史 flush~~ | 已落地 |
| 2 | ~~Phase 2 终答投影~~ | 已落地 |
| 3 | ~~Phase 3 控制面收敛~~ | 已落地 |
| 4 | ~~Phase 4 按行渲染~~ | 已落地（标题着色首刀） |
| 5 | ~~Phase 5 导出~~ | 已落地 |
| 6 | Phase 6 自动化回归 | 可并行：金样/CI |

**推荐下一刀**：Phase 6（SSE fixture → TUI 投影行序金样）。

---

## 7. 验收清单（维护者）

- [x] 多工具「分析当前项目」类回合：流式中旁白在工具前；结束后刷新仍如此（Phase 1）。  
- [x] 工具批结束后终答在工具之后、不与旁白双显（Phase 2）。  
- [x] 常规回合控制面无成对工具噪音（Phase 3）。  
- [ ] `cargo test --lib golden_web_v2_row_order_preserved_in_tui_projection_block` 保持绿；新增历史/终答测例挂 CI。  
- [ ] 变更 turn-layout / 投影文案时同步 **`docs/Turn布局设计.md`** 与本文「已落地」表。

---

## 8. 变更检查清单

- [ ] 改 `turn_project` / 投影块格式 → 更新本文 Phase 状态 + Turn 布局文档交叉链接  
- [ ] 改工具展示 → `message_display` + tool-card 金样 / hydrate fixture  
- [ ] 改导出 projection → **`docs/命令行与路由.md`**、`crabmate-chat-export` 信封说明  
- [ ] 改 TUI 模块边界 → **`docs/开发文档.md`** 模块索引（若增删 `mod`）

---

*维护约定：实现完某一 Phase 后，把本节对应行标为已落地并依赖 Git 历史追溯；勿在本文堆长篇 changelog。*
