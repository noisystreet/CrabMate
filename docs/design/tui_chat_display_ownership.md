# TUI 中区展示所有权（ADR）

> **历史文档（已归档）**：本仓同进程 TUI 已于 **D2.2 硬删**（见 [`client_shell_split.md`](./client_shell_split.md) §2.5）。官方终端为 Client **`crabmate-tui`**。下文决策仅适用于已删除的 `src/runtime/tui/`。

**状态**：~~已采纳（2026-07）~~ **已归档**（实现随 D2.2 移除）  
**受众**：考古时查阅  
**关联**：[`tui_align_tauri_display.md`](./tui_align_tauri_display.md)、[`../Turn布局设计.md`](../Turn布局设计.md)

## 背景

终端 TUI 对齐 Tauri 行序时，曾同时拼接 **定稿 transcript、Turn 投影、LLM scratch 流式尾**，并用前缀匹配 /「投影非空跳过全部 assistant」等启发式防双显。结果是正文滞后、标签闪烁、定稿丢答等一串展示 bug。

## 决策

**本轮中区 content 的权威源是 Turn 投影**（`crabmate-turn-layout` + `TuiTurnProjection`）。

| 角色 | 职责 |
|------|------|
| **Turn 投影** | 进行中与定稿的旁白 / 工具 / 终答行序；open 段对 scratch 做 live catch-up |
| **`owns_streaming_content_lane`** | 投影已拥有 content 时，流式尾**不再**挂 `scratch.content` |
| **`build_tui_chat_body`** | 中区唯一合成入口：`committed transcript` + 投影 + 控制面附录 +（可选）流式尾 |
| **`CommittedTurns` flush** | 有可定稿布局 → user 前缀 + 投影块 + **仅投影未覆盖**的 plain assistant；仅 timeline → 回退 Message[] |
| **Message[]** | 会话真相、导出、会话切换 reseed；**不是**进行中跟字的主路径 |
| **scratch** | 喂投影 live / 终答捕获；仅在投影未拥有 lane 时作为流式尾 |

## 非目标

- 不复刻 Web DOM / CSS / 分区组件。
- 不把 Leptos `TurnLayout` 状态机搬进 ratatui。
- 不新增第四条「旁路拼串」路径（新需求应改投影或 `build_tui_chat_body`）。

## 后果

- 展示类修复优先改 `turn_project` / `build_tui_chat_body` / flush 覆盖判定，避免再加全局 `should_hide` 特例。
- Web 与 TUI 继续共享 `Turn` / `project_turn_web_v2` 契约；像素与交互可分叉。
