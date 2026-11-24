# 移交摘要（handoff brief）：DTO 形状与上界

> **状态**：**已落地**（`e40f0f29`）：`TaskState.files`（schema 2 → 3）+ 分区预算 brief + 6 条单测，CI 全绿。
> **受众**：维护者、后续实现者。§9 的两条开放问题已结清（一条不做、一条转为可调常量）。
> **关联**：[context_window_compaction.md](context_window_compaction.md)（ADR：压缩 + 基线化 + 归档）、
> [subagent_delegation.md](subagent_delegation.md)（L2 已落地的窄契约）、
> [multi_agent_collaboration.md](multi_agent_collaboration.md) §0-1 / §5（`context_slice` 的目标形状）。

## 1. 结论

跨窗口续接缺的**只有一个字段**：本会话被工具写过哪些文件。
本期给 `TaskState` 加 `files: Vec<String>` 并渲染进 `render_task_brief`；
**不**新建统一 `HandoffBrief`，**不**改 L2 委托契约，**不**实现完整的 `context_slice`。

## 2. 问题：三处口径里只有一处是真缺口

issue #1496 列了三处「移交该带什么」，核对后只有第三条成立：

| 口径 | 内容 | 判定 |
|---|---|---|
| `TaskState.evidence` | 证据指针 `id`/`kind`/`tool`/`ok`/`exit_code` | **真缺口**：无任何文件路径 |
| `SubTaskReport.key_paths` | 文件路径 ≤20，由 changelist 差集填充 | 不缺文件；缺「试过什么 / 哪次失败」 |
| `WorkspaceChangelist` | 触碰路径 + per-file diff，上限 12000 字符 | 不缺，但体量比 brief 大一量级 |

`task_evidence_refs`（`turn_loop/outer_loop.rs`）从工具消息归一而来，**不带工具入参**，
因此推不出路径；`key_paths` 已由 `agent/delegation.rs` 的 changelist 前后差集填充。
把后者当成「没有文件」去补，等于重做一遍。

**未登记的第二处落差**（不属本 DTO）：`agent_autonomy_enhancement.md` 称 `TaskState` 存
「子项 → evidence_refs」映射，而实际 `TaskState` **没有子项列表**。需另立 Issue，见 §10。

## 3. 主流做法（2026-11 核对官方文档）

| 框架 | 子段看到什么 | 移交摘要形态 | 机器供给工作区状态 |
|---|---|---|---|
| OpenAI Agents SDK | **默认全量历史**，`input_filter` 才裁 | 摘要段 + `<CONVERSATION HISTORY>` 包裹 | 无 |
| Claude Code subagent | **全新上下文**；父写任务消息 | 自然语言 delegation message | **有：`git status` 快照** |
| LangGraph | 由 state schema 决定，checkpointer 恢复 | 「Summarize messages」节点自定义 | 无 |

**取值参考（2026-11 核对官方文档）**：

| 实现 | 参数 | 取值 | 计量单位 |
|---|---|---|---|
| Aider（repo map） | `--map-tokens` | 默认 **1k tokens** | token 预算 |
| Claude Code | `autoCompactWindow` | **≥100000 tokens**（低于此值无效） | token 绝对量 |
| Claude Code | `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` | 按**百分比**覆盖触发点 | 上下文比例 |
| Claude Code | subagent 描述合计 | 超 **15000 tokens** 启动警告 | token 预算 |
| OpenAI Agents SDK | `nest_handoff_history` | 摘要段，无公开条数/字符上限 | — |

两点：主流一律用**预算（token / 字符 / 百分比）**表达上限，**没有一家用「N 条」**；
Aider 原文允许「通常不超，但有时显著超出」——即**软上界**，不是硬砍。
未取得的三个源（当时不可达，**不猜**）：OpenHands condenser 的 `max_size` / `keep_first`、
Gemini CLI 的压缩阈值、LangChain `trim_messages` 的默认参数。

三条直接可用的结论：

1. **我们的选择有主流背书**：子回合全新 messages + 只回 summary，与 Claude Code 非 fork
   subagent 同构；父写 `prompt` 即其 delegation message。OpenAI 敢默认传全量，是因为同进程同
   provider；我们跨后端（MiniMax 把 `system` 合进 `user`），所以 §0-1 结构化是对的。
2. **Claude Code 给了硬通道**：除父写的任务消息外，harness 还注入 `git status` 快照。我们这边
   的等价物是 changelist，且 `CrabmateDelegator` 已持有该句柄——**不需要**改 `SubTaskRequest`。
   本期不做，列为 §7 的 P2。
3. **摘要化不脱敏是一类真实故障**：OpenAI 明确写「工具参数/输出可能留在生成的摘要里」，
   并提醒只过滤 `input_items` 会漏。这正是 `task_state` 不变量 1（只存指针、不存正文）要防的，
   **不要**因为对方保留工具项就跟着放行。
4. **上限用预算表达，条数只作副闸**：见上表，主流无一家用条数；故 D3 里 files 段以**字符**为主闸、
   **条数**为副闸。Aider 的「软上界」也说明：宁可受控超支，也不要静默截断掉别的段。

## 4. 决策

### D1 不新建统一 `HandoffBrief`

新建统一类型要同时改 `SubTaskReport` 与 `TaskState`，即动 L2 已验收契约，且两者截断口径不同
（4000 字符 + 20 路径 vs 全文件 800 字符）。本期只加能立刻被读的那个字段。
将来若做 L3 的 `context_slice`，本字段可直接映射为 `context_slice.files`，语义同源，不算返工。

被否：*让 brief 直接引用 changelist 的 diff 正文*——需在 800 内再裁一刀，且会把更多仓库正文写进状态文件。

### D2 `files` 来源是 changelist，不解析工具入参

唯一通用来源是 `WorkspaceChangelist::touched_paths()`，由工具层写盘时的 `record_mutation` 累积：
每加一个新写工具都自动覆盖。按工具名解析 `Args` 要维护提取器，且 `run_command` 的入参根本无法
可靠提取——那是安全面（AGENTS §4.6）扩大，明确不做。与 `key_paths` 同源，两处口径自然一致。

### D3 上界：800 字符内分区预算，字符为主闸

| 位置 | 上限 | 理由 |
|---|---|---|
| `TaskState.files`（落盘） | 20 条 | 落盘不占上下文，此处条数是可移植单位 |
| brief 的 files 段 | **≤200 字符**，且 ≤5 条 | 字符是主流计量单位；条数作副闸防「5 条超长路径」 |
| 单条路径 | 不单独截断 | 路径短，段闸兜底即可 |

**超支处理顺序（不许静默丢信息）**：先缩 files 条数 → 再缩「最近证据」末条 → 段尾附「（已压缩）」。
注意现状 `render_task_brief` 是「拼完再截 800」，新增段会**静默挤掉末尾证据行**；
本期须一并改为**分区预算**（否则 `goal` 取满 400 时 files 段只剩约 140 字符，必然挤掉证据）。

保留方向：取 changelist `order`（**首次**触碰顺序）的**尾部** 20 条。
`order` 重复写入不重排，故这是「最后碰过」的**近似**——不为此新增时间戳字段。

### D4 schema 演进走既有惯例

`TASK_STATE_SCHEMA` 2 → 3；`files` 带 `#[serde(default)]`。读取仍只拒未来版本，
旧文件读出空列表 ⇒ brief 少一行、不报错、不迁移。

上界在**唯一赋值路径**施加：新增 `with_files(files)`（与 `with_cumulative` 同风格），内部走
`cap_files`——**不改 `new_paused` 签名**（那会波及测试等既有调用点，且让构造器继续单参）。
与 `cap_evidence` 同范式：写出去的永远不超界。

### D5 父→子不改契约（本期）

「`prompt` 是唯一入口、父上下文一概不传」是**已验收的安全边界**，不是遗留缺陷；加字段属改 ADR。
要让子任务知道父已动过哪些文件，走 §7 的 P2 硬通道，不新增 `SubTaskRequest` 字段。

### D6 只保留一个路径来源

续跑 brief 与将来的子回合注入**共用** changelist 快照，不各自造口径。
这是本仓已踩过的坑（同一件事三套口径），此处显式记为约束。

## 5. DTO 与渲染

```rust
// src/cm_agent/task_state.rs
pub const TASK_STATE_SCHEMA: u32 = 3;
/// `files` 条数上限（与 `SubTaskReport::MAX_KEY_PATHS` 对齐）。
pub const MAX_TASK_FILES: usize = 20;
/// brief 里 files 段的**字符**上限（主闸，主流用预算不用条数）。
pub const TASK_BRIEF_FILES_MAX_CHARS: usize = 200;
/// brief 里 files 段的**条数**上限（副闸，防少数超长路径把段撑爆）。
pub const TASK_BRIEF_FILE_ITEMS: usize = 5;

pub struct TaskState {
    // ...既有字段不变...
    /// 本会话内被工具写入触碰过的工作区相对路径（首次写入顺序）。
    /// 旧文件无该键 ⇒ 空（`#[serde(default)]`），brief 少一行，不报错。
    #[serde(default)]
    pub files: Vec<String>,
}
```

`render_task_brief` 在「累计已用」之后、「最近证据」之前增一行：

```text
已改动文件（最近 5 个）：src/a.rs、src/b.rs、…（另 3 个，完整清单见会话变更集）
```

段内先按 `TASK_BRIEF_FILES_MAX_CHARS` 装填、再按 `TASK_BRIEF_FILE_ITEMS` 收口；被收口的条数以
「（另 N 个，完整清单见会话变更集）」如实告知（**不是**静默截断）。「（已压缩）」只用于 §8 第 5 条那种
整段都塞不下的硬截断，不标记正常收口。

## 6. 接线点

| 阶段 | 位置 | 改动 |
|---|---|---|
| 写 | `turn_loop/outer_loop.rs::record_budget_paused` | 取 changelist 快照 → `.with_files(files)` |
| 渲染 | `cm_agent/task_state.rs::render_task_brief` | 增「已改动文件」行 |
| 读 | `run_agent_turn.rs`（读 `state` → 渲染 brief） | 不变 |
| 剥落盘 | `server_injected_user::strip_runtime_refreshed_injections` | 不变 |

写入侧取快照用 `context::agent_turn_prep::workspace_changelist_for_turn(cfg, process_handles, scope)`——
已有现成函数，**不要**给 `RunLoopCore` 加字段（那是公开面）。
落地后确认：`record_budget_paused` 直接读 `p.ctx.obs.process_handles`，**无需**给 `RunAgentTurnAttach`
新句柄。

**已知边界（不是缺陷，登记以免误判覆盖面）**：`TaskState::new_paused` 的生产调用点只有预算拒绝这一处
（`turn_loop/outer_loop.rs`），因此只有**预算暂停**会落 `files`；`TaskPauseReason::AwaitingApproval`
的暂停不写 `TaskState`，自然也没有 `files`。将来若把审批暂停也纳入续跑（#1386 S2），需在同一路径补取快照。

## 7. 与 `messages` 的边界

- `files` 是落盘 JSON 字段；渲染进 brief 时才成为文本。不新增 `user.name` 注入种类，
  不改 `is_paused_task_injection` 的判定。
- 不触碰 §0-1「契约不承载内部表示」：本期不把 `files` 放进任何跨 Agent 契约。

**P2（不在本期，需单独 PR）**：子回合注入工作区变更快照，对标 Claude Code 的 `git status`。
方案：`CrabmateDelegator::run` 装配子 messages 时多注入一条结构化 `user` 消息（只读快照，
≤N 条路径），**不改 `SubTaskRequest`**。前置：需同步修订 `subagent_delegation.md` §5 的
「`session.messages` 只含收窄 system + 任务描述」与该文 §8 的表述——这是一次契约文本变更，
必须单独评审，不能夹带在本期里。

## 8. 验收

1. `files` 超 20 条 → 保留尾部 20；`cap_files` 有单测。
2. 旧 schema 文件（无 `files` 键）读入 → 空列表、brief 正常渲染。
3. changelist 关闭 / 工作区未设置 → `files` 为空、**不 panic**（fail-open，与落盘同口径）：
   写入侧用 `unwrap_or_default()`，无独立单测；「无 `files` 时 brief 不多出一段」由
   `legacy_schema_without_files_still_loads` 覆盖。
4. brief 总长 ≤ 800：`long_goal_never_crowds_out_evidence_line` 用取满 `goal` 的用例钉住。
5. files 段超 200 字符 → 条数回退并附「另 N 个」，**证据段不被挤掉**
   （`files_segment_is_char_bounded_before_item_cap` / `files_segment_total_length_respects_char_cap`）。
6. 触发 files 段压缩时，「最近证据」仍保留 ≥1 条（降级循环的下界写死为 1）。
7. 手工：预算耗尽后 `.crabmate/tasks/<scope>.json` 含 `files`，brief 含「已改动文件」行。

## 9. 开放问题（已结清）

1. **§7 的 P2 本期不做**：它是**契约文本变更**（要改 `subagent_delegation.md` §5 / §8），
   必须单独评审、单独 PR，不能夹带在实现 PR 里。边界记在 §7 与 §10。
2. **200 字符 / 5 条仍未经实测校准**，按主流做法取的是**初始值**：字符为主闸、条数为副闸。
   两者都是 `task_state.rs` 的常量（`TASK_BRIEF_FILES_MAX_CHARS` / `TASK_BRIEF_FILE_ITEMS`），
   实测路径长度分布后直接改常量即可，**不需要**动 schema 或契约。**不照抄** Aider 的 1k tokens
   或 Claude Code 的 100k——前者是整仓 repo map、后者是压缩触发点，与「跨回合 brief」语义不同。

## 10. 不做

- 不实现完整 `context_slice`（`facts[]` / `input_artifacts[]` 无生产消费者）。
- 不改 `SubTaskReport` 与 L2 委托契约（D5）。
- 不引入「换干净窗口」形态（#1496 明确非目标）。
- `files` 不参与权限 / 审批判定——纯提示信息，安全面零变化。
- **`TaskState` 的子项列表**另行立 Issue：它牵动验收反接（S1 已放弃）与 L6，量级不同。
