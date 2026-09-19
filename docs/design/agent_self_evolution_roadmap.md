# 设计文档：Server Agent 自进化能力增强路线

**状态**：草案（待评审；评审后再定实施顺序）

**关联文档**：
[`summarize_experience.md`](./summarize_experience.md)（已实现的经验沉淀工具）、
[`summarize_experience_todo.md`](./summarize_experience_todo.md)（该工具的待扩展项）、
[`memory_todo.md`](./memory_todo.md)（记忆系统扩展全集）、
[`../待办清单.md`](../待办清单.md)（长期记忆检索质量 / 生命周期 / 运营 API 条目）、
[`tool_calling_evolution.md`](./tool_calling_evolution.md)（工具调用层演进，非本文范围）。

---

## 1. 目标与非目标

### 1.1 目标

把当前的"长期记忆 + 经验沉淀"从**会话内的文本存储**升级为**可跨会话复用、可用效果数据驱动、可自我纠错**的闭环，使 agent 在持续使用中相对初始版本变得更好，且这一改善**可被量化验证**。

### 1.2 非目标

- **不做**模型权重层面的训练 / 微调（本仓库只对接 OpenAI 兼容 `chat/completions`）。
- **不做**让 agent 无约束地改写自身配置或源码（见 §4.4 的受限方案与 §8 拒绝项）。
- **不重复** `summarize_experience_todo.md` / `memory_todo.md` 中已有的单点设计；本文只做**分层优先级与依赖编排**，具体字段设计回链原文档。

---

## 2. 现状（代码事实）

已落地的"自进化"骨架是一条**不完整**的闭环：

| 环节 | 实现 | 位置 |
|---|---|---|
| 模型自主沉淀经验 | `summarize_experience`（≥20 字符、去重、共用写入路径） | [`long_term_memory_tools.rs`](../../src/cm_internal/long_term_memory_tools.rs#L95-L132) |
| 系统启发式自动沉淀 | 仅识别「构建/验证类工具由失败转成功」 | [`auto_summarize_experience.rs`](../../src/cm_memory/memory/auto_summarize_experience.rs#L1-L20) |
| 回合结束自动索引 | `user`/`assistant` 终答正文入库（可配 `long_term_memory_auto_index_turns`） | [`long_term_memory.rs`](../../src/cm_memory/memory/long_term_memory.rs) |
| 召回注入 | 经验源优先 + 标签/关键词加权 | [`long_term_memory_recall.rs`](../../src/cm_memory/memory/long_term_memory_recall.rs#L19-L40) |
| 存储 | 会话级 SQLite，列含 TTL / 标签 / 来源 | [`long_term_memory_store.rs`](../../src/cm_memory/memory/long_term_memory_store.rs#L42-L72) |
| 自我认知 | `self_config_info`（**只读**，19 小节） | [`self_config_info.rs`](../../src/cm_tools/tools/self_config_info.rs#L37) |
| 工作区备忘 | `.crabmate/agent_memory.md` 首轮注入（**需人工编辑**） | [`agent_memory.rs`](../../src/cm_memory/memory/agent_memory.rs) |
| 效果评估 | benchmark + LLM-as-Judge | [`e2e_scenario.rs`](../../src/e2e_scenario.rs#L767) |

配置面见 [`config/memory.toml`](../../config/memory.toml)（作用域、向量后端、条目上限、注入预算、top-k、TTL、自动索引 / 自动沉淀开关、经验优先召回开关）。

---

## 3. 瓶颈分析

按"是否卡住进化闭环"排序：

### 3.1 作用域锁死，经验出不了会话（结构性阻塞）

[`config/memory.toml`](../../config/memory.toml#L7) 与 [`long_term_memory.rs`](../../src/cm_memory/memory/long_term_memory.rs#L3) 均明确：作用域固定为 `conversation_id`。
后果：**换一个会话，此前积累的经验不可见**。经验复用率趋近于零，长期使用不会让 agent 变好——这是当前"自进化"最本质的缺口。

### 3.2 无效果反馈，无法判断经验是否有用

[`MemoryRow`](../../src/cm_memory/memory/long_term_memory_store.rs#L14-L22) 无 `hit_count` / `confidence` 等字段；召回权重是写死的常量（经验 `+0.25`、经验源 `+0.1`、标签命中 `+0.2`、关键词重叠 `×0.15`，见 [`long_term_memory_recall.rs`](../../src/cm_memory/memory/long_term_memory_recall.rs#L19-L40)）。
后果：注入后是否被采纳、是否真的解决了问题，全无记录。**没有反馈信号就没有"进化"可言**，也无法淘汰噪音。

### 3.3 无质量护栏，可能越学越差

- 无冲突消解（`supersedes`）：两条方向相反的经验会同时注入（[`summarize_experience_todo.md`](./summarize_experience_todo.md) §2.2）。
- 无时效判定：TTL 是唯一手段，代码库/依赖变化导致的"过时但未过期"无法识别。
- 无置信度：偶然成功的做法与广泛验证的模式等价占预算（[`memory_todo.md`](./memory_todo.md) §13.2）。

### 3.4 沉淀覆盖面窄

[`auto_summarize_experience.rs`](../../src/cm_memory/memory/auto_summarize_experience.rs#L45-L103) 仅覆盖 build/test/format 类工具的失败→恢复，且靠关键词表（`user_message_suggests_task`）触发。调试、性能、工具调用失败恢复等同样有沉淀价值的过程未被覆盖。

### 3.5 行为不可自适应

`self_config_info` 只读；system prompt / 工具描述是静态 Markdown。运行期证据（哪类经验有用、哪类工具描述误导模型）无法反馈到行为层。

---

## 4. 增强方向

五个层次，**依赖关系自下而上**：A 是 B 生效的前提，C 是 B 安全运行的前提。

### 4.1 A 层：打通作用域（结构）

**设计**：引入分层 scope，并在检索时做 union（**决议见 §7-1：只做两级，不引入 `user:`**）：

```
conversation:{conversation_id}   ← 现有语义，保留
workspace:{workspace_root_hash}  ← 项目级，跨会话共享
```

- 检索：按 `conversation` → `workspace` 优先级合并候选，再走现有 [`pick_recall_chunks`](../../src/cm_memory/memory/long_term_memory_recall.rs#L65) 排序与预算裁剪。
- 写入：`summarize_experience` 增加作用域参数（会话级 / 项目级），默认会话级（向后兼容）。
- 已有设计：`summarize_experience_todo.md` §3.3、`memory_todo.md` §5.1（`workspace_style_remember`）。

**信任前提（必须写进实现与文档）**：`workspace` 作用域隐含「同一工作区 = 同一信任域」。多租户场景下**不得**让不同租户共用同一工作区作用域，须靠每租户独立实例 / 独立工作区隔离（见 [`../未来规划功能.md`](../未来规划功能.md#L9-L22)「模式 A」）。

**落点**：`prepare_messages` 的 `scope_id` 入参（[`long_term_memory.rs`](../../src/cm_memory/memory/long_term_memory.rs#L234-L245)）、store 的 scope 查询、`LongTermMemoryToolState.scope`。

**风险**：跨会话共享会放大噪音与隐私面。缓解：工作区级写入默认需显式意图（如 `workspace_style_remember`），并受 §4.3 护栏约束。

**验收**：同一工作区两个会话，会话 B 能召回会话 A 沉淀的项目级经验；`long_term_memory_store` 有对应单测。

### 4.2 B 层：效果反馈闭环（让"进化"名副其实）

**设计**：

1. **schema 扩展**：新增 `hit_count INTEGER DEFAULT 0`、`last_hit_unix INTEGER`、`confidence TEXT DEFAULT 'medium'`（走已有的 `ensure_column` 幂等迁移，见 [`long_term_memory_store.rs`](../../src/cm_memory/memory/long_term_memory_store.rs#L24-L39)）。
2. **注入回写**：`prepare_messages` 注入后更新命中计数与时间戳。
3. **有效性追踪**：回合内记录被注入经验与后续工具调用结果的关系——经验建议被印证（工具成功）则强化，被明显违背/失败则弱化。参考 `summarize_experience_todo.md` §3.5 与 `memory_todo.md` §9.3。
4. **数据驱动权重**：把 [`experience_recall_boost`](../../src/cm_memory/memory/long_term_memory_recall.rs#L19-L40) 中的固定常量改为按 `hit_count × 有效率` 调权，低价值经验降权并在超预算时先被裁剪。

**风险**：回写增加 SQLite 写放大；"是否印证"的判定可能误报。缓解：判定只在置信度足够的信号上做（工具退出码、build/test 结果），失败则保守不更新。

**验收**：给定历史命中数据，召回排序随数据变化；新增 `long_term_memory_stats` 类只读接口可查看命中分布。

### 4.3 C 层：质量护栏（防止倒退）

按 `summarize_experience_todo.md` §2.2 / §4.2 与 `memory_todo.md` §13.1 落地：

- **冲突消解**：`supersedes_id` 字段；注入时过滤已被替代的经验。
- **时效判定**：经验涉及的文件/依赖版本 vs 当前工作区比对，标注"可能过时"或降权。
- **会话起始自检**：可选 `review_experiences`，对比当前上下文判断已注入经验是否仍有效，产出修改建议（缩短 TTL / 建立替代关系），系统按建议执行或交用户确认。
- **写入频率上限**：`summarize_experience` 每回合上限（`summarize_experience_todo.md` §3.2），防记忆噪音。

**风险**：自检本身消耗 LLM 预算。缓解：配置开关 + 每回合有界调用，默认 fail-open。

### 4.4 D 层：行为自适应（受限的"自我修改"）

分两档，先做低风险档：

- **软性档（优先）**：把"经验"作为行为调节通道——工作区风格记忆（`workspace_style_remember`，`memory_todo.md` §5.1）注入 system prompt 层，实现**不改代码即改变行为**。
- **参数档（谨慎）**：仅允许 agent 在**白名单、低风险、可回滚**范围内调整少量参数（如沉淀触发引导的开关/阈值），需复用现有 `tool_approval` 审批与审计路径，且变更必须落盘可追溯。

**非目标**：不允许 agent 直接改写源码 / 结构配置 / prompt 文件正文。理由见 §8。

### 4.5 E 层：扩大沉淀面

把 [`auto_summarize_experience.rs`](../../src/cm_memory/memory/auto_summarize_experience.rs) 的启发式从单一 build-recovery 扩展到 debug / test / 性能 / 工具失败恢复等模式。

**准入判据（决议见 §7-5）**：以**是否存在机器可验证信号**划界，而非"是否需要模型调用"。

- **允许自动沉淀**：工具退出码由非零转零、测试 `fail → pass`、benchmark 指标改善、重试后成功等确定性信号触发。这类是对**事实的结构化记录**，不是"提炼"；现有 build-recovery 即此先例。
- **仍拒绝**：无验证信号、仅靠语义归纳的自动提炼（`summarize_experience_todo.md` §8 原意）。
- **硬性约束**：E 层每新增一种模式，**必须绑定一个确定信号**，否则不扩展。
- **可选**：在此判据之外，也可改为回合末由模型判断是否沉淀（有模型调用，不属于 §8 拒绝项），但需另定预算与开关。

---

## 5. 实施顺序建议（待评审确认）

| 阶段 | 内容 | 依赖 | 说明 |
|---|---|---|---|
| P0 | 本文评审定稿 | — | 只出方案，不写码 |
| P1 | A 层分层 scope | — | 收益最高、是后续一切的前提；须同时落实 §4.1 信任前提 |
| P2 | B 层效果反馈 | P1 | 让召回权重数据驱动，为 C 提供判定信号 |
| P3 | C 层护栏 | P1、P2 | 规模变大后必须有的防退化机制 |
| P4 | D 层软性档（工作区风格记忆） | P1 | 复用已有记忆通道，风险低 |
| P5 | E 层沉淀面扩展 | P2 | 需先有护栏，否则噪音放大 |
| — | D 层参数档 | P3 全部完成 | 高风险，需单独评审 |

---

## 6. 验证方法（如何证明"变强了"）

"自进化"必须可量化，否则无法判断增强是否有效：

1. **回归对照**：用已有 benchmark 流程（`--benchmark` + `--batch`，支持 SWE-bench / GAIA / HumanEval / Generic）跑「关闭记忆」vs「开启记忆」两组，比较任务成功率。
2. **LLM-as-Judge**：复用 [`e2e_scenario.rs`](../../src/e2e_scenario.rs#L767) 的评分能力，做纵向对照（同一批任务，记忆池积累前后）。
3. **记忆质量指标**：命中率分布、注入后任务成功率、被淘汰条目的占比——通过 B 层新增的统计接口观测。
4. **防退化门禁**：C 层护栏上线后，回归对照不应出现下降；出现下降则视为护栏失效。

---

## 7. 决议与开放决策

### 7.1 已定决议

**§7-1 作用域粒度（已定）**：只做 `conversation` + `workspace` 两级，**不引入 `user:`**。
依据：[`../未来规划功能.md`](../未来规划功能.md#L9-L22) 已达成共识——不在进程内做账号体系、鉴权不区分自然人，身份交由前置网关/BFF。连"可选演进"也止步于「多枚服务级 API Key → 租户 id 映射」，故 `user:` 无落地载体。若将来真做租户，应新增服务级的 `tenant:{service_key_id}`，而非 `user:`。实现须同时落实 §4.1 的**信任前提**（同一工作区 = 同一信任域）。

**§7-5 E 层原则边界（已定）**：以「是否存在机器可验证信号」划界。
依据：[`summarize_experience_todo.md`](./summarize_experience_todo.md#L320-L328) §8 拒绝的是「对普通对话无信号做语义提炼（无需模型调用）」，而非"系统自动落盘"本身——[`auto_summarize_experience.rs`](../../src/cm_memory/memory/auto_summarize_experience.rs) 的 build-recovery 已是先例。故：有确定信号 → 允许（结构化记录事实）；无信号 → 拒绝。E 层每新增模式必须绑定一个确定信号。详见 §4.5。

### 7.2 待定开放决策

1. **跨会话默认行为**：工作区级经验的写入是模型自主，还是默认需用户显式触发？
2. **B 层判定信号**：有效性追踪允许付出多少额外 LLM 调用？是否接受"保守更新（宁可不更新）"的取舍？
3. **D 层边界**：是否允许 agent 改动任何运行期参数？若允许，白名单范围如何界定？
4. **存储选型**：规模上去后是否仍用单文件 SQLite，还是按 `../待办清单.md` 长期记忆条目接入 FTS5 / 外部向量库？

---

## 8. 明确不做（拒绝项）

| 条目 | 原因 |
|---|---|
| 模型权重微调 / 自训练 | 超出本仓库"OpenAI 兼容后端调用方"的定位 |
| agent 无约束改写源码、结构配置或 prompt 文件正文 | 不可控、不可审计，且违背仓库既有安全边界 |
| 无验证信号的自动提炼（仅靠语义归纳，无需模型调用） | 质量不可控，违背 `summarize_experience_todo.md` §8 核心设计原则；有确定信号的系统自动沉淀不受此限（见 §7.1 §7-5） |
| 为自进化单独引入外部代理 / 消息队列 | 当前为单进程模型，与 `../待办清单.md` P5「跨进程队列」同批再议 |
