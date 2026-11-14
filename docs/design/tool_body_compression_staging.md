# 工具正文压缩的分级与时机（#1327 设计）

> 状态：**方案待评审**。对应 Issue #1327（工具正文压缩：最新一条也被压、且不看窗口余量）。
> 本文只讨论「何时压、压哪一条、如何可观测」，不改已定的预算口径（trigger / target / safety）。

## 1. 结论

采纳 issue 的 **A + C**，并补一条 issue 未列、但 A 案必需的前提：

- **A'**：`compress_tool_message_contents` 保留最近 `MIN_RECENT_TURN_GROUPS`（=2）组不压，只压更旧的 `tool` 正文。
- **兜底**：当压缩后仍超 `max_input_tokens`（硬上限）且已无可删整组时，对最近组再压一次——
  否则 A' 会把「大输出 + 小窗口」变成请求被上游拒绝。这是 A' 的**前置条件**，不是可选项。
- **C**：把本档命中的条目（索引 / 原始字符数 / 保留头尾）记进 `context_trim` 的 `detail`。
- **不做 B**（上限随窗口余量缩放）；理由见 §5。
- 附带清理：同步管道里**第二次** `compress_tool_message_contents` 调用（`sync_pipeline.rs:176`）是无效遍历，删除。

## 2. 现状（已核实）

### 2.1 管道顺序

`prepare_messages_before_model_call_sync_with_budget`（`src/agent/context_window.rs:177`）直接改调用方的
`&mut Vec<Message>`——即**会话 canonical**，不是视图副本。所以本档压缩**不可逆**：正文被采样后原文不再存在，
只能靠 `output_original_chars` 这类元数据知道「曾经有多大」。

`apply_session_sync_pipeline_with_config`（`src/cm_agent/message_pipeline/sync_pipeline.rs:119`）内顺序：

| 步骤 | 锚点 | 说明 |
|---|---|---|
| ① 压 tool 正文 | `sync_pipeline.rs:134` | 无条件、全部 `role=tool`，上限 `tool_message_max_chars` |
| ② 按条数裁剪 | `:146` | `max_message_history` |
| ③ 按字符预算裁剪 | `:161` | `context_char_budget`；配了 `llm_context_tokens` 时该预算被置 0（`context_window.rs:164`） |
| ④ 再压一次 tool | `:176` | **无效遍历**：①已把正文压到 ≤ 上限，裁剪不会让正文变长 |
| ⑤ 剔除孤儿 tool | `:189` | |

Token 分级在另一处、更晚执行（`compact_messages_to_token_budget`，`src/agent/context_compaction.rs:442`）：

```
仅在 token_triggered（before 或 after 的 used_input_tokens > trigger_tokens=85%）时：
  若 after > target_tokens(70%) → compress_old_tool_messages(留最近 2 组, 压到 2000)
  然后 remove_old_groups_to_target（留最近 2 组）
```

### 2.2 默认参数决定触发面很窄

| 键 | 默认 | 位置 |
|---|---|---|
| `command_max_output_len` | **8192**（clamp 1024..8MiB） | `src/cm_config/finalize/agent_layers.rs:125` |
| `tool_message_max_chars` | **32768**（clamp 1024..1MiB） | `src/cm_config/finalize/tail.rs:140` |
| `llm_context_tokens` | 262144 | `config/default_config.toml:14` |
| `context_token_trigger_percent` / `target` / `safety` | 85 / 70 / 2048 | `config/default_config.toml:37-39` |

**默认配置下 ① 档不命中**：单条 `run_command` 输出最多 8192 字符，远小于 32768。
命中需要满足其一：

- 用户把 `command_max_output_len` 调大（大文件 `cat`、宽条件 `grep -rn` 场景，最大可到 8 MiB）；
- 非 `run_command` 的工具正文超过 32 KiB（大响应 `http_fetch`、archive 预览等，各自有独立上限）。

这条很重要：**#1327 不是「默认路径上的普遍问题」，而是「用户为了拿全信息主动调大输出上限后，正好在最需要
完整输出的那一次被采样」**。issue 的严重度判断（正确性受影响）成立，但优先级取决于这些场景的使用频率。

### 2.3 采样质量与可观测性

- 截断本身不差：`maybe_compress_tool_message_content`（`src/cm_tools/tool_result/mod.rs:408`）对 envelope 形状
  走 `tool_output_head_tail_sample`，并写 `output_truncated` / `output_original_chars` / `output_kept_*`。
  非 envelope 形状退化为前缀截断（`:427`）。
- 幂等：采样后 `output` ≤ 上限，再次调用返回 `None`（`:411` 早退）。所以 ④ 是纯浪费。
- 观测：本档命中会置 `has_trim()`（`cm_agent/context_timeline` 的 `ContextTimelineAcc`，含 `compress_hits > 0`），
  发 `context_trim`，`detail` 里只有 `compress_hits` **计数**（`ContextTimelineAcc::trim_event` 构造的
  `TrimDetail`，见 [`context_timeline_detail.md`](context_timeline_detail.md)）——
  客户端水位据此回落，但「哪一条被压了、压掉多少」无处可查。

## 3. 对 issue 三条问题的核实结论

| issue 说法 | 核实 |
|---|---|
| P1 最新一条也被压，影响正确性 | **成立**，且比 issue 描述的更重：压缩写回 canonical，原文永久丢失 |
| P2 分级顺序反了 | **部分成立**。不是顺序反（先粗压再细压本身合理），而是 ① 档缺少「保留最新」语义；`DEGRADE_TOOL_MAX_CHARS=2000` 必须显著小于 32768 的约束（`context_compaction.rs:19-26`）说明两档是配合关系 |
| P3 完全不看窗口余量 | **成立，但不建议按 issue 的 B 案修**，见 §5 |

## 4. 方案 A'（推荐）

### 4.1 改动

1. `transforms.rs` 的 `compress_tool_message_contents` 增加 `keep_recent_groups: usize`，内部取
   `recent_groups_start(messages, keep_recent_groups)` 为边界，只遍历 `[..boundary]`。
2. `recent_groups_start` 目前是 `context_compaction.rs:286` 的私有函数 → 移到 `cm_agent/message_pipeline`
   （与 `conversation_turn_groups`、`remove_oldest_turn_group` 同处），两边共用，`MIN_RECENT_TURN_GROUPS`
   也一并搬到该模块，避免「压缩保留 2 组、删组保留 2 组」两个常量漂移。
3. 调用点传 `MIN_RECENT_TURN_GROUPS`；**删除** `sync_pipeline.rs:176` 的第二次调用。
4. 若某次调用想恢复旧行为（全压），传 `0` 即可——不需要新配置键。

### 4.2 为什么按「组」而不是「最后一条」

- `tool` 消息按协议是紧跟 `assistant(tool_calls)` 的响应，一条 `assistant` 可能带多个 `tool_calls`；
  只跳过「最后一条 tool」会留下同组的其余被压正文，模型看到的同批结果口径不一致。
- 与 token 分级的 `MIN_RECENT_TURN_GROUPS` 同口径，解释成本最低：「最近两轮永远完整，更旧的先降 2000，再删组」。

### 4.3 必需的兜底（否则 A' 引入回归）

A' 之后，最新组的 `tool` 正文保持原样（最大可达用户的 `command_max_output_len`，最高 8 MiB）。
现有链路对此只有一条 warning（`context_window.rs:303-310`：压缩已尽力仍超硬上限），**没有强制压缩**。
于是「用户把输出上限调到 1 MiB + 32k 窗口的本地模型」会直接把请求打爆。

兜底放在 `finalize_token_budget_compaction`（`context_window.rs:277`）内、删组之后：

```text
if compaction.after.used_input_tokens > budget.max_input_tokens
   && compaction.removed_turn_groups == 0 {
    // 已无可删整组，说明超限来自最近组本身：这时才允许压最近组
    compress_tool_message_contents(messages, cfg.tool_transcript.tool_message_max_chars, 0);
}
```

即「保留最近组」是**软保护**，硬上限面前让路——和现有 `tail_guard` 在「尾部自身就撞硬上限」时的放行
（`context_compaction.rs:440-441` 注释）是同一套取舍。

## 5. 为什么不做 B（上限随窗口余量缩放）

issue 的 B 案：`effective_max = min(32768, 剩余输入预算 × x%)`。不采纳的原因：

1. **量纲不匹配**：预算器算的是 token（`count_prompt_tokens_*`），① 档在字符层。用固定 `x%` 把 token
   预算折成字符，等于自造一个 `chars_per_token` 常数——CJK 与英文能差 2~4 倍，猜错就是「该压时不压」或「不该压时压」。
2. **已有正确的预算控制**：token 分级（trigger 85% / target 70% / DEGRADE 2000 / 删组）就是干这件事的，
   且它**看真实 token、只动旧组**。① 档的正当职责是「单条正文的重发成本上限」，与窗口无关——
   一条 32 KiB 的 tool 正文在 1M 窗口里依然会被重发 N 次，成本与窗口大小无关。
3. **窗口未知时的退化行为难定义**：`llm_context_tokens = 0` 时回退 `context_char_budget`（2 万字符），
   这时 `x%` 分子分母都不存在，B 案需要再造一套退化规则。

若未来仍想要 B，正确做法是**在 token 层**做（例如「单条 tool 正文的 token 数上限」），而不是给字符上限乘百分比。

## 6. 方案 C（可观测性）

`compress_tool_message_contents` 返回值从 `usize` 改为一个小结结构：

```rust
struct ToolCompressHit { index: usize, original_chars: usize, kept_head: usize, kept_tail: usize }
```

汇总进 `MessagePipelineDelta`（`sync_pipeline.rs:52`）→ `ContextTimeline` → `context_trim` 的 `detail`
新增 `tool_compressed: [...]` 数组（保留 `compress_hits` 计数以求兼容）。

**这是线协议变更**（AGENTS §4.8）：`detail` 是 SSE `context_trim` 的 payload，新增字段属兼容扩展，但必须
同步 `docs/SSE协议.md`、`fixtures/` golden 与 Client 仓 `parser_v2.rs` / `sse_dispatch/types.rs`。
如果这一轮只想修正确性，C 可拆到后续 PR：A' + 兜底不触碰协议。

## 7. 行为变化

| 场景 | 现状 | A' 之后 |
|---|---|---|
| 默认配置（输出 ≤ 8192 < 32768） | ① 档不命中 | 不变（A' 是 no-op） |
| 调大 `command_max_output_len`，最新组有超限正文 | 被采样，原文丢失 | 最新组完整；更旧组照旧压到 32768，近窗口时再降到 2000 |
| 同上，且最新组本身就超 `max_input_tokens` | 采样后可能进得去 | 兜底压最近组（与现状等效），但**先**尝试保留完整 |
| 配置了 `tool_message_max_chars` 很小的老部署 | 全部压到小值 | 最近 2 组不再被压，可能比现状更占预算；靠 token 分级与兜底收敛 |

第四行需要在 PR 描述里点明：**这是可感知的行为变化**，尽管默认值下无差别。

## 8. 测试计划

- 单测（`transforms.rs`）：最近 2 组的 `tool` 正文不动；第 3 组起被压到上限；`keep_recent_groups = 0` 等价旧行为。
- 单测（`sync_pipeline.rs`）：管道跑完后 `delta.tool_compress_hits` 只统计旧组；④ 删除后计数不变。
- 单测（`context_compaction.rs`）：最近组超硬上限时兜底压缩生效；未超限时不压（避免无谓损失）。
- 集成/golden：`turn-completion-golden`、`acceptance-golden` 不应变化（默认配置不命中）。
- 手工复现（issue §Reproduction）：`command_max_output_len = 65536`，跑一次 `cat` 大文件，确认
  下一轮 `context_trim` 的 `compress_hits` 不再因最新一条而置位。

## 9. 落地顺序

1. **PR-1（正确性）**：`recent_groups_start` / `MIN_RECENT_TURN_GROUPS` 搬家 + A' + 删 ④ + 兜底 + 单测。
2. **PR-2（可观测性）**：C，含 `docs/SSE协议.md` 与 Client 侧契约同步。
3. 可选：把 `DEGRADE_TOOL_MAX_CHARS=2000` 与 `tool_message_max_chars` 的关系写进 `docs/配置说明.md`
   （现在只写在代码注释里）。

## 10. 非目标

- 不改 `context_token_trigger_percent` / `target_percent` / `safety_margin_tokens`。
- 不改客户端 `ctx` 水位语义（`max` 只增、`context_trim` 回落）。
- 不新增配置键（A' 的「保留组数」固定为常量；确有需要再议）。
- 不解决「最新一条被采样后模型如何得知可重跑」的提示语问题（属提示词层面）。
