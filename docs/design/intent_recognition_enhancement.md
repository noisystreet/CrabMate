# CrabMate 意图识别能力增强设计

**状态**：设计稿（可分阶段落地）  
**受众**：维护者、Agent 架构贡献者、质量与运营同学  
**关联**：`src/agent/intent_router.rs`、`src/agent/hierarchy/router.rs`、`docs/DEVELOPMENT.md`

---

## 1. 背景与问题

CrabMate 当前已经具备两层“路由/意图”能力：

- **首轮快路径意图路由**：`src/agent/intent_router.rs` 将输入分类为 `Greeting` / `Qa` / `Execute` / `Ambiguous`，并基于阈值决定 `DirectReply`、`ConfirmThenExecute`、`Execute`。
- **任务复杂度路由**：`src/agent/hierarchy/router.rs` 根据规则或 LLM 判断任务复杂度，选择 `single/react/hierarchical/multi_agent` 执行模式。

现状能够覆盖基础场景，但在以下方面存在明显提升空间：

| 问题 | 现状表现 | 影响 |
|------|----------|------|
| 分类粒度偏粗 | `Execute` 合并了“代码修改、诊断、文档、提交”等多类执行意图 | 下游策略难以精细化控制 |
| 主要依赖关键词 | 对隐式请求、跨句省略、多意图句子识别不足 | 误判率、追问率偏高 |
| 上下文利用不足 | 主要按单轮文本判断，未充分利用会话状态/近期操作 | 连续对话稳定性不足 |
| 置信度未校准 | 当前分数是启发式累加，不是统计意义上的可信概率 | 阈值难统一管理与演进 |
| 观测与回灌薄弱 | 缺少标准化“意图判定日志 + 失败样本回流”机制 | 难以持续优化 |

---

## 2. 设计目标与非目标

### 2.1 目标

1. **识别更准**：提升真实用户请求下的意图分类准确率与拒识质量。  
2. **决策可控**：建立统一的置信度阈值、澄清策略与高风险二次确认。  
3. **可观测可迭代**：形成离线评测、线上指标、失败样本回灌闭环。  
4. **渐进兼容**：不破坏现有路由逻辑，先增量接入，再逐步替换。  

### 2.2 非目标

- 首版不追求一次性引入复杂 DSL 或通用规则引擎。
- 首版不要求替换所有现有路由器；允许双轨运行（旧逻辑兜底）。
- 不在本设计中定义完整产品文案，仅定义文案模板接口与策略。

---

## 3. 总体方案：四层意图识别流水线

### 3.1 架构概览

```
用户输入 + 会话上下文
        |
        v
[L0 预处理与特征抽取]
  语言、关键词、文件路径、命令痕迹、最近工具行为
        |
        v
[L1 快速路由(规则)]
  greeting/qa/明显执行请求快速命中
        |
        v
[L2 细粒度意图分类(LLM或小模型)]
  主意图 + 次意图 + slots + confidence + abstain
        |
        v
[L3 策略决策]
  直接执行 / 先澄清 / 二次确认 / 仅回复
        |
        v
执行层(单Agent / 分层 / 多Agent) + 可观测日志
```

### 3.2 意图层级（建议）

建议将现有 `Execute` 拆为可控子类（示例）：

- `execute.code_change`：改代码/重构/实现功能
- `execute.debug_diagnose`：排查报错/复现问题/定位根因
- `execute.run_test_build`：运行测试、构建、命令验证
- `execute.docs_ops`：文档整理、注释、说明更新
- `execute.git_ops`：`commit`/分支/PR 相关操作
- `qa.explain`：解释概念/报错/机制
- `qa.compare_or_advice`：方案对比与建议
- `meta.greeting` / `meta.smalltalk`
- `unknown` / `multi_intent`

说明：一级意图用于流程路由，二级意图用于权限和策略控制。

---

## 4. 核心模块设计

### 4.1 L0：预处理与特征抽取

输入：

- 当前用户消息
- 最近 N 轮对话摘要（建议 3~6 轮）
- 近期执行痕迹（最近工具类型、是否失败、是否在澄清流程中）

输出特征（示例）：

- `has_file_path`、`has_command`、`has_git_keyword`
- `contains_error_signal`（error/panic/traceback）
- `is_short_utterance`（如“帮我看下这个”）
- `language`、`has_mixed_intents`

作用：为 L1/L2 提供上下文增强，降低单轮文本误判。

### 4.2 L1：快速规则路由

目标：低成本处理高确定性场景，缩短路径并稳定体验。

策略：

- 明确寒暄、纯能力询问 -> 直接答复
- 明确高风险动作关键词（如“删除、覆盖、强推”）-> 直接标记需确认
- 命中率高的固定表达 -> 直接路由

要求：

- 规则可配置（热更新优先）
- 每条规则可打点（命中率、误判率）
- L1 只做“高置信 shortcut”，其余请求交给 L2

### 4.3 L2：细粒度意图分类

可选实现：

1. **LLM 结构化分类（MVP 首选）**：输出 JSON，开发成本低。  
2. **Embedding 召回 + 重排**：提升稳定性与可解释性。  
3. **专用分类模型**：高 QPS/低成本场景后续引入。  

统一输出契约（建议）：

```json
{
  "primary_intent": "execute.debug_diagnose",
  "secondary_intents": ["execute.run_test_build"],
  "confidence": 0.82,
  "abstain": false,
  "need_clarification": false,
  "clarification_reason": "",
  "slots": {
    "target_files": ["src/agent/intent_router.rs"],
    "expected_outcome": "定位并修复首轮路由误判"
  }
}
```

关键点：

- 支持 `abstain`（拒识）与 `need_clarification`（需追问）分离。
- 支持多意图句子（`secondary_intents`）供后续拆分执行。
- `slots` 仅保存对路由有帮助的关键参数，避免过度耦合执行细节。

### 4.4 L3：策略决策层

根据 `intent + confidence + risk + context` 进行最终路由：

- **直接执行**：高置信、低风险、上下文充分
- **先澄清**：中低置信或关键槽位缺失
- **确认后执行**：高风险动作（如 git 写操作、删除、覆盖）
- **仅回复**：问答/解释/闲聊类

默认阈值建议（可配置）：

- `execute_direct_threshold = 0.75`
- `execute_confirm_threshold = 0.55`
- `< 0.55` 且非问答：走澄清

---

## 5. 数据与评测体系

### 5.1 数据集构建

建议按三层数据组织：

- **训练集**：历史真实消息 + 合成补齐
- **验证集**：用于调阈值与提示词
- **回归集**：线上误判样本沉淀（必须长期维护）

标注字段建议：

- `primary_intent`, `secondary_intents`
- `need_clarification`, `risk_level`
- `slots`（可选）
- `final_expected_route`（direct/clarify/confirm/reply）

### 5.2 离线指标

- 意图分类：`Accuracy`、`Macro F1`、`Top-2 Recall`
- 路由质量：`Route Accuracy`
- 拒识质量：`Abstain Precision/Recall`
- 澄清质量：`Clarification Hit Rate`（澄清后可执行比例）

### 5.3 线上指标

- `误执行率`（本不应执行却执行）
- `无效澄清率`（澄清后仍无法执行）
- `用户改写率`（同一请求被迫重复表达）
- `任务首轮成功路由率`
- `端到端完成时长`（受澄清链路影响）

---

## 6. 与现有代码的集成建议

### 6.1 模块边界

- `src/agent/intent_router.rs`
  - 保留现有快路径逻辑，抽象为 `L1FastIntentRouter`
  - 新增 `IntentDecision` 统一结构体，替代分散返回类型
- 新增建议：`src/agent/intent_pipeline.rs`
  - 编排 L0/L1/L2/L3
  - 对外只暴露 `assess_and_route(...)`
- `src/agent/agent_turn/hierarchy.rs`
  - 接入统一 `IntentDecision`，减少分支散落
- `src/agent/hierarchy/router.rs`
  - 消费“细粒度执行意图”，优化模式选择（如 debug 类优先 hierarchical）

### 6.2 配置项（建议）

可在配置中新增意图段（命名仅供讨论）：

```toml
[agent.intent]
enable_pipeline = true
l2_provider = "llm"              # llm | embedding | classifier
execute_direct_threshold = 0.75
execute_confirm_threshold = 0.55
context_turn_window = 4
enable_multi_intent_split = true
```

---

## 7. 上线计划（4 个阶段）

| 阶段 | 目标 | 主要产出 |
|------|------|----------|
| Phase 0 | 可观测先行 | 统一埋点、意图日志、失败样本收集 |
| Phase 1 | MVP 管道 | L0+L1+L2(LLM) + L3 决策接入，旧逻辑兜底 |
| Phase 2 | 稳定性增强 | 阈值校准、多意图拆分、澄清模板优化 |
| Phase 3 | 成本与性能优化 | Embedding/专用分类器引入、缓存与批处理 |

发布策略建议：

- 灰度开关（按会话或用户分桶）
- 保留一键回退到旧路由
- A/B 对比“首轮成功率 + 误执行率 + 平均完成时长”

---

## 8. 风险与缓解

| 风险 | 说明 | 缓解 |
|------|------|------|
| 过度澄清 | 阈值偏保守导致交互冗长 | 分场景阈值 + 澄清模板优化 |
| 误执行 | 阈值偏激进导致越权或误动作 | 高风险动作强制确认 + 白名单 |
| 分类漂移 | 新功能上线后语义边界变化 | 每周回灌样本 + 回归集门禁 |
| 成本增加 | L2 增加额外模型调用 | L1 提前命中 + 缓存 + 分层降级 |

---

## 9. 验收标准（建议）

满足以下条件可认为“意图识别增强一期完成”：

1. 统一 `IntentDecision` 契约已接入主链路。  
2. L2 分类可返回 `abstain/need_clarification`。  
3. 线上可看到意图与路由指标，并支持按版本对比。  
4. 回归集评测可在 CI 或定期任务中运行。  
5. 相比基线，误执行率下降且首轮成功路由率提升。  

---

## 10. 后续可扩展方向

- 基于用户历史行为的个性化阈值（保守/激进模式）。
- 跨语言意图适配（中英混输、术语映射）。
- 意图与权限策略联动（按工具类别与风险级别）。
- 在 `agent_reply_plan` 生成阶段复用意图结果，减少重复推断。

