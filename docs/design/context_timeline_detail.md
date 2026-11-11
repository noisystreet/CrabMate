# 上下文时间线事件的类型化（#1347 S4 设计）

> 状态：**S4a 已实施**（`ContextTimelineDetail` 类型化，wire 与 canonical 字节不变）；
> **S4b 已决策不做**（`detail` 是 7 个 `timeline_log` 生产点共用的字段，改动远超本设计初估，见 §2.2）。
> 对应 Issue #1347 的 S4。
> 本文只解决 `context_trim` / `context_inject` 的 `detail` 靠约定、没有编译期约束的问题；
> 用量口径见 [context_usage_single_source.md](context_usage_single_source.md)。
>
> 基线：`main` @ `69176f0d`（S2 已合入 main）。

## 1. 现状与问题

`ContextTimelineEvent` 的 `detail` 是 **String**：

```rust
// src/cm_agent/context_timeline/mod.rs
pub struct ContextTimelineEvent {
    pub kind: &'static str,
    pub title: String,
    pub detail: String,        // 字符串化的 JSON
}
```

> S4a 之后该字段是 `ContextTimelineDetail`；上面的 `String` 是**类型化之前**的形态，保留作对照。

`trim_event` / `inject_event` 用 `serde_json::json!{ … }.to_string()` 手工铺开（`trim_event` 共 21 个字段，
其中 `tool_compressed` 还要手写一层 map）。由此产生四个问题：

1. **字段集合靠约定**：漏字段、改名、类型写错都在运行期才暴露，编译器帮不上忙。
   历史上 #1345 就是这一类（`history_removed` 起初只隐含在 `title` 文案里）。
2. **字符串化 JSON**：wire 上 `TimelineLogBody.detail` 是 `Option<String>`
   （[protocol.rs](../../src/cm_sse_protocol/sse/protocol.rs)），消费方必须先 `JSON.parse` 再取值。
3. **canonical 里存的是同一份字符串**：`timeline_marker()` 把 detail 原样写进
   `crabmate_timeline` 消息 → **历史会话全是旧形状**。任何改形状的方案都必须能读旧数据，
   不是「改 Client 解析器」就完事。
4. **title 分流残留在 Client**：Server 侧 `trim_title()` 只用 `history_removed` 选**文案**
   （`已裁剪历史` / `已压缩工具输出`），`control_classify` 按 key 存在性分流，都不看 title；
   但 Client 的 `crabmate-client-api`（`timeline_skill.rs`）仍兼容「title\n<detail>」拼接形态。

## 2. 决策

### 2.1 S4a：Server 内部类型化，**wire 字节不变**

```rust
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ContextTimelineDetail {
    Trim(TrimDetail),
    Inject(InjectDetail),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TrimDetail {
    pub count_hit: bool,
    pub char_hit: bool,
    pub n_before: usize,
    pub n_after: usize,
    pub compress_hits: usize,
    pub tool_compressed: Vec<ToolCompressHit>,   // 不再是手写 json! 映射
    pub summarized: bool,
    pub history_removed: bool,
    pub tail_kept: Option<usize>,
    pub before_tokens: Option<u32>,
    pub after_tokens: Option<u32>,
    // …与类型化之前的 21 个 `json!` 键一一对应（顺序即键顺序）
}
```

约束：

- `ContextTimelineEvent.detail` 改为 `ContextTimelineDetail`；
- **只在 encode 边界**做 `serde_json::to_string`：SSE 的 `TimelineLogBody.detail` 与 `timeline_marker()`。
  wire 与 canonical 的字节形状**保持不变**（仍是字符串化 JSON）；
- 读历史 marker 用 `serde_json::from_str::<ContextTimelineDetail>`，所有字段 `#[serde(default)]`
  以吸收旧数据缺字段；
- 加一条**防漂移测试**：同一 `ContextTimelineAcc` 状态下，类型化实现序列化出的 detail 字符串
  与旧实现**逐字相同**。这比 golden 更强，因为 golden 只覆盖到夹具里的那几种状态。

收益：21 个手写字段变成 struct 字段（漏一个编译不过）；`tool_compressed` 由结构体直接序列化；
Client、`SSE_PROTOCOL_VERSION`、两个 golden **全都不动**。

### 2.2 S4b：wire 变 JSON object —— **已决策：不做**

`TimelineLogBody.detail` 不是 `context_*` 专用字段，而是**所有 `timeline_log` 事件共用**的。
本仓有 7 个生产点：

| 位置 | 事件 |
| --- | --- |
| `src/run_agent_turn.rs` | 终答相关旁注 |
| `src/agent/agent_turn/host/execute/tools/emit.rs` | `tool_result_summary` |
| `src/agent/agent_turn/turn_loop/orchestration_route.rs` | 路由旁注 |
| `src/agent/agent_turn/turn_loop/context_timeline_sse.rs` | `context_trim` / `context_inject` |
| `src/cm_sse_protocol/sse/final_response_terminal.rs` | 终态（`detail: None`） |
| `src/cm_sse_protocol/sse/web_approval.rs` | `approval_decision` |
| `src/cm_sse_protocol/sse/ag_ui_convert.rs` | AG-UI 投影（`detail: None`） |

把 `Option<String>` 改成 `Option<Value>` 的真实成本：

1. 7 个生产点全改——其中审批、工具摘要的 `detail` 是**纯文本**，要包成 `Value::String`
   或 `{"text": …}`；
2. `replay_detail_value` 的「纯文本 → `{"text": …}` 包装」回退分支（`context_timeline_sse.rs`，
   有 3 条测试守着）语义失效，要么删、要么改成异构判断；
3. Client 三个解析点改签名（`context_axis.rs` 的 `context_trim_after_used_tokens` /
   `context_trim_history_removed` / 水位回落，`timeline_skill.rs` 的注入识别）；
4. **旧 canonical marker 里 `detail` 是字符串**，读侧必须永久双形状兼容；
5. `SSE_PROTOCOL_VERSION` bump + 两个 SSE golden + 两份文档。

收益只有一个：Client 少一次 `serde_json::from_str::<Value>`（微秒级）。

**结论：不做。** 改为在 `docs/SSE协议.md` 写明「`context_trim` / `context_inject` 的 `detail`
是服务端类型化 JSON（`ContextTimelineDetail`）的字符串形态，字段集见本设计」——
Server 侧的类型化收益（S4a）已经拿到，wire 不用动。
若将来仍要做，须单开 issue 并带上面这份成本清单；写入侧切换**必须晚于**双形状兼容读上线。

### 2.3 title 分流的收尾

- **Server**：`trim_title()` 保留（纯文案），注释写明「不含语义，语义看 `history_removed`」。
  现在 `history_removed` 已是字段，文案选择不再承担契约。
- **Client（另一个 PR）**：`timeline_skill.rs` 去掉「title\n<detail>」拼接兼容分支。
  注意旧 canonical 数据仍在，所以是改成「按 kind 判定」而不是「按 title 猜」，
  不是简单删代码。

## 3. 实施步骤（S4a，已实施）

落地形态：新子模块 [`context_timeline/detail.rs`](../../src/cm_agent/context_timeline/detail.rs)
（`TrimDetail` / `InjectDetail` / `ContextTimelineDetail`），`context_timeline.rs` 同步改为目录
（`mod.rs` + `detail.rs`），`MAX_TOOL_COMPRESS_DETAIL` 提到模块级。

1. 定义 `ContextTimelineDetail` 与两个 detail struct；字段顺序与类型化前的 `json!` 一一对应。
2. `ContextTimelineEvent.detail` 改类型；`inject_event` / `trim_event` 改为构造 struct。
3. 只在 encode 边界 `to_json_string()`（SSE `context_timeline_sse` 与 `timeline_marker` 两处）。
4. 两条测试：`detail_json_bytes_match_golden`（字节形状，夹具
   [context_timeline_detail_golden.json](../../fixtures/context_timeline_detail_golden.json)）、
   `legacy_detail_string_parses_into_trim_detail`（旧 marker 缺字段兼容）。
5. `docs/SSE协议.md` **不需要**改（形状没变）。

## 4. 验收

1. `trim_event` / `inject_event` 里不再出现 `serde_json::json!` 手写字段；
2. 「同一状态序列化出的 detail 字符串与旧实现逐字相同」测试存在且会红（改任一字段名即红）；
3. 旧 marker 兼容单测通过；
4. `fixtures/context_compaction_golden.json`、SSE golden、`cargo test --workspace` 全绿；
5. Client 零改动即可运行（用现有 Client 版本对接验证一次）。

## 5. 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| 序列化字段顺序变化 | `serde_json::json!` 保插入序，struct 保声明序；顺序变了会让「逐字相同」测试红，但线上无害 | 恰恰要靠这条测试把顺序钉住，避免无意的顺序漂移 |
| `#[serde(untagged)]` 歧义 | 两个 detail struct 字段集不同，正常不会误匹配；但若未来加同名字段要重新评估 | 用 `kind` 显式 tag 亦可（需要 detail 里带 kind，会改 wire）——本设计选 untagged 以保字节不变 |
| 旧数据缺字段 | 历史 marker 可能没有新字段 | 全部 `#[serde(default)]`，并在兼容测试里喂最老形状 |
| 有人顺手把 wire 也改了 | S4a 的边界就是「字节不变」 | 防漂移测试 + PR 描述写明不做 S4b |

## 6. 待拍板

1. **已决**：拆成子模块 `context_timeline/detail.rs`（原文件 752 行，加上类型定义与测试会贴到 920 行棘轮）。
2. **已决**：防漂移测试用 **fixture 快照**（`fixtures/context_timeline_detail_golden.json`），
   不在测试里保留第二份 `json!` 对照实现。
3. **仍开放**：Client 侧去 title 分流的 PR 何时发？（可晚于 S4a，不阻塞）
