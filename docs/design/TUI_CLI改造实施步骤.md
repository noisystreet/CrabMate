# CLI → TUI 改造实施步骤（与 Web 布局对齐）

本文面向维护者：在**不替换既有 Agent / 工具 / 配置栈**的前提下，将当前 **reedline 行式 REPL** 渐进演进为 **全屏 TUI**，并在**信息架构**上与 Web 侧栏 + 主会话区 + 状态/辅助区对齐。

**现状简述（仓库当前代码）**

- 交互式 CLI 入口在 **`src/runtime/cli/repl.rs`**，输入由 **`repl_reedline`**（reedline）驱动；配色与提示集中在 **`cli_repl_ui`**，底层已有 **`crossterm`**（根 **`Cargo.toml`**）。
- 已有 **`src/runtime/tui/`**（**`crabmate tui`**）与 **`ratatouille`**（**`crossterm`** 后端）；当前以占位 UI + 终端恢复为主，对话闭环见阶段 C。配置里 **`tui_session.json` / `tui_load_session_on_start`** 等仍为后续数据化预留语义（见 **`docs/开发文档.md`** 对 **`workspace_session`** 的说明）。

**原则**

1. **编排复用**：会话消息、`run_agent_turn`、工具执行、审批与 **`CliToolRuntime`** 等与现有 REPL/`chat` 子命令共用；TUI 仅替换「渲染 + 输入 + 焦点」层（与 Web 前端的职责划分对称）。
2. **三端约定**：新能力优先落在共享层；若某能力明确仅适合某一界面，须在注释或 **`docs/待办清单.md`** 中写明。参见仓库 **`.cursor/rules/cli-tui-web-shared-logic.mdc`**。
3. **非 TTY 降级**：管道、CI、`ssh -T` 等场景**不得**假设全屏 TUI；保留 **`repl`**（或 **`chat` 单次**）作为 fallback。
4. **对齐 Web 的是「分区」而非「控件」**：左侧 ≈ 工作区/列表，中间 ≈ 消息流 + 输入，右侧或底栏 ≈ 任务/状态/快捷入口；不要求像素级一致。

---

## 技术选型建议

| 方向 | 建议 | 说明 |
|------|------|------|
| 布局与组件 | **`ratatouille`**（推荐） | 与现有 **`crossterm`** 常见组合成熟；社区示例多，便于实现分区与滚动列表。 |
| 终端控制 | **`crossterm`** | 已在依赖中；注意 **`NO_COLOR`**、Windows Terminal、tmux、SSH 下按键差异。 |
| 异步与 UI | **独立线程 + channel** 或 **`tokio`** 与 UI tick 分离 | `run_agent_turn` 多为 async；避免在渲染线程里阻塞；事先约定 **取消**（Ctrl+C / 组件 drop）与 **日志**（勿向 alt screen 混打 `println!`）。 |

可选：**Cargo feature** `tui` 挂载 **`ratatouille`**，便于裁剪发行版（与现有 **`mcp` / `docker_sandbox` / `fastembed`** feature 策略一致则需更新 **`docs/开发文档.md`** 与 **`README.md`**）。

---

## Web → TUI 布局映射（概念）

| Web | TUI 对应（建议） |
|-----|------------------|
| 左侧：工作区 / 会话 / 导航 | 左栏固定宽度 **`List` / `Tabs`** |
| 中部：消息气泡 + 输入框 | 中部 **`Paragraph` 滚动区** + 底部 **`Input` / `TextArea`** |
| 右侧：任务、状态栏、设置入口 | 右栏可折叠，或 **首版用底栏 + Modal** 降低复杂度 |
| 顶栏 | **状态行**（模型、工作区缩写、快捷键提示）或 **`:` 命令模式** |

首版可采用 **两栏 + 底栏**，待稳定后再加常驻右栏。

---

## 分阶段实施

### 阶段 A：脚手架与入口

**目标**：可启动、可退出、resize 不崩，不占死终端。

- 新增子命令，例如 **`crabmate tui`**（或与 **`repl --tui`** 二选一，需在 **`docs/命令行与路由.md`** 与 **`README.md`** 写明）。
- 新建模块目录（示例）：**`src/runtime/tui/`**（并在 **`src/runtime/mod.rs`**、必要时 **`src/lib.rs`** / **`cli`** 路由注册；合并后按 **`.cursor/rules/architecture-docs-sync.mdc`** 更新 **`docs/开发文档.md`** 模块索引）。
- 实现：**raw mode**、**alt screen**（若使用）、**Drop 时恢复终端**、**Ctrl+Q / Ctrl+C** 退出策略。
- 验收：TTY 下进入全屏占位 UI；非 TTY 直接报错退出或提示使用 **`repl`** / **`chat`**。

### 阶段 B：静态布局壳（对齐 Web 骨架）

**目标**：屏幕分区固定，数据可先占位。

- 使用 **`Layout` / `Flex`** 划分：左（窄）· 中（主）·（可选）右；底部 **`status_bar`**。
- 文案占位：中间「消息区」、左侧「工作区」可先写死一行。
- 尊重 **`NO_COLOR`**；可选降级为无 truecolor 主题。
- 验收：终端缩放后布局可用；无残留乱码光标。

**进展（仓库）**：布局渲染在 **`runtime/tui/run_session.rs`**（**`render_*`**）：**顶栏一行（CrabMate · 模型 · `api_base` · 工作目录）+ 三栏（约 23% / 54% / 23%，贴近 Web `nav-rail` 与主区）+ 底栏快捷键**，区块标题对齐 Web「会话在左 / 工作区在右 / 聊天 / 撰写」语义（左「会话」、右「工作区」含快捷键）；尊重 **`NO_COLOR`**；缩放随 **`Terminal::draw`** 重算。

### 阶段 C：对话闭环（最小可用）

**目标**：TUI 内完成一问一答，走与 REPL **同一套**业务入口。

- 从 **`runtime/cli/chat.rs`** / **`run_agent_turn_for_cli`** 等提取或复用「组装 **`Vec<Message>`** + 工作目录 + **`CliToolRuntime`**」的调用链，避免在 TUI 内复制一套编排。
- **输入**：先单行提交即可；输出以 **增量追加** 为佳（若 CLI 已有流式回调，接到 **UI 线程安全队列** 再刷新列表）。
- 验收：能调用模型与工具（工具输出可先纯文本块）；与 **`--no-stream`** 行为差异在文档中说明。

**进展（仓库）**：**`runtime/tui/run_session.rs`** 的 **`run_tui_session`** 在 **`cli_run`** 与 **`repl`** 相同的配置/客户端/工具装载之后运行；ratatouille 在专用线程，异步侧调用 **`repl_prepare_messages_and_editor`** 与 **`repl_dispatch_chat_round`**；**`CliTerminalChatBuildArgs::suppress_stdout_render`**（库根装配）关闭助手 stdout 渲染，**`--no-stream`** 仍控制上游是否 SSE；中区由 **`messages_to_transcript`** 刷新。**`/api-key`**（**`/apikey`**）经 **`try_dispatch_api_key_slash_for_tui`** 写入 transcript；其它 **`/`** 前缀提示使用 **`repl`**（**不**发往模型）。

### 阶段 D：左侧栏数据化（工作区 / 会话）

**目标**：与 Web / CLI **会话文件形状**一致。

- 复用 **`runtime/workspace_session`**、**`.crabmate/tui_session.json`**、**`chat_export`** 等与 **`docs/命令行与路由.md`**「CLI 与 Web 能力对照」一致的契约。
- **`/` 命令**：要么保留单行前缀解析（与 **`repl_slash_dispatch`** 同源），要么改为 **`:` 命令模式**；须统一写入 **`README`** 与 **`/help`** 类文案。

### 阶段 E：审批、任务与设置（对齐产品行为）

**目标**：不在「只有 Web 能点同意」上倒退。

- **`run_command` 等审批**：复用 **`tool_approval`** / **`CliApprovalInput`**；TUI 侧用 **Modal / 底栏菜单** 呈现选项（与 **`dialoguer`** 行式菜单相比，更贴近 Web「确认条」心智）。
- **任务列表**：若 Web 有 **`GET /tasks`** 等价需求，CLI/TUI 应通过已有 **`fetch`** 能力或进程内状态（视架构而定）对齐，避免私自造第二套任务模型。
- **设置**：终端不适合复制 Web 全表单；可先做 **只读摘要 + 打开编辑器 / `config reload` 说明**，与 **`/config`**、**`docs/配置说明.md`** 交叉引用。

### 阶段 F：打磨与发布策略

- **键盘**：焦点在侧栏 / 主区 / 输入之间切换（**Tab** / **hjkl** 等），**`?` / F1** 帮助。
- **大会话**：虚拟滚动或截断显示 + 「展开全文」。
- **默认入口**：是否将 **`crabmate`** 无子命令时导向 TUI，属于产品决策；建议长期保留 **`repl`** 作为脚本与 SSH 友好默认。
- **文档**：更新 **`README.md`**、**`docs/命令行与路由.md`**、**`docs/开发文档.md`**（模块索引与架构图）；若有新配置键或 **`CM_*`**，同步 **`docs/配置说明.md`**（见 **`.cursor/rules/todolist-and-documentation.mdc`**）。

---

## 风险与规避

- **日志与 tracing**：全屏模式下默认应避免向 stdout 刷结构化日志；使用文件日志或显式 **`RUST_LOG`** 策略。
- **Windows / SSH**：先在本机主流终端验证，再在 README 标注推荐环境。
- **勿在第一版移除 REPL**：避免破坏自动化与窄环境用户。

---

## 参考路径（仓库内）

- **`src/runtime/cli/repl.rs`**：主循环与 slash 分支。
- **`src/runtime/cli/chat.rs`**：`run_agent_turn_for_cli` 等 CLI 回合入口。
- **`src/runtime/cli_repl_ui.rs`**：ANSI 与 **`NO_COLOR`** 约定。
- **`src/runtime/workspace_session.rs`**：会话加载与保存。
- **`src/tool_approval/`**：CLI 审批抽象。
- **`docs/Web界面美化设计.md`**：Web 分区与样式层级（TUI 对齐语义分区即可）。
- **`docs/命令行与路由.md`**：子命令与用户可见行为。

本文仅作路线图；具体接口命名、crate 边界与 PR 拆分由实现阶段按需调整。
