# Prompt Caching 支持 — 设计文档

**状态**：设计草案（v2，审校修正）  
**日期**：2026-07-09  
**受众**：维护者、架构决策者  
**关联**：
- `agent_space/open_source_agent_comparison.md` — 对比分析（P0: Prompt 缓存）
- `crates/crabmate-types/src/chat_api.rs` — ChatResponse / StreamChunk 类型定义
- `crates/crabmate-llm/src/api/mod.rs` — HTTP 调用入口（流式+非流式）
- `crates/crabmate-llm/src/api/sse_parser.rs` — SSE 解析（usage 在最后一帧）
- `crates/crabmate-llm/src/retry.rs` — 重试引擎
- `src/runtime/diagnostic_summary.rs` — 诊断摘要

---

## 1. 背景与动机

### 1.1 现状

CrabMate 主力使用 DeepSeek API（V4-Flash / V4-Pro），**DeepSeek 已全面支持自动上下文硬盘缓存**[^1]，但 CrabMate 当前完全没有利用：

- `ChatResponse` 未解析 `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
- 流式 SSE 路径中 usage 被静默丢弃（`StreamChunk` 无 `usage` 字段；`choices:[]` 时跳过）
- 不报告缓存命中率或成本节省估算
- 无日志/指标暴露缓存效果

### 1.2 缓存工作原理

DeepSeek 的上下文缓存是**自动且透明的**[^2]（V4 新增"公共前缀检测"）：

1. **结束位置落盘**：每次请求结束时，用户输入 + 模型输出分别落盘为缓存前缀单元
2. **公共前缀检测**：多次请求间检测到公共前缀时，独立落盘该公共部分（无需用户干预）
3. 后续请求完整匹配前缀单元时命中缓存
4. 缓存按 **\$0.014/1M**（V4-Flash）计费，为常规价格的 10%
5. 5 分钟窗口，每次命中刷新 TTL

**关键：V4 的公共前缀检测使多轮对话和同前缀请求天然受益，即使消息顺序不完全一致。** 优化重点应放在 system prompt 内容稳定性上，而非消息重排。

### 1.3 预期收益

| 场景 | 命中率估算 | 成本节省 |
|------|-----------|---------|
| 仅可观测化（不改消息结构） | 20-30% | 18-27% |
| + system prompt 稳定性优化 | 40-60% | 36-54% |
| + 显式 `cache_control` | 60-80% | 54-72% |

---

## 2. 设计目标

### 2.1 核心目标

1. **可观测**：捕获并报告缓存命中率（流式+非流式双路径）
2. **可优化**：system prompt 内容稳定性（而非消息重排）
3. **可配置**：供应商感知的缓存策略
4. **向后兼容**：不影响现有功能

### 2.2 非目标

- 客户端缓存（仅利用服务端缓存）
- Anthropic 的显式 `cache_control` breakpoints（CrabMate 目前不支持 Anthropic）
- LLM 响应去重/批处理（正交功能）
- 消息重排优化（DeepSeek 公共前缀检测已覆盖，且工具定义在请求的 `tools` 字段而非 `messages`）

---

## 3. 实施阶段

### Phase 0：缓存统计可观测化（3-5 天）

#### 3.1 `ChatResponse` / `Usage` 类型扩展

**文件**：`crates/crabmate-types/src/chat_api.rs`

DeepSeek 返回的 JSON 结构（缓存字段在 `usage` 对象内）：

```json
{
  "choices": [...],
  "usage": {
    "input_tokens": 1500,
    "output_tokens": 300,
    "prompt_cache_hit_tokens": 1200,
    "prompt_cache_miss_tokens": 300
  }
}
```

新增类型：

```rust
/// DeepSeek `usage` 对象中的缓存统计（非 DeepSeek 供应商可能不返回）。
#[derive(Debug, Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// 本次请求输入中缓存命中的 token 数。
    pub prompt_cache_hit_tokens: Option<u64>,
    /// 本次请求输入中缓存未命中的 token 数。
    pub prompt_cache_miss_tokens: Option<u64>,
}
```

`ChatResponse` 增加 `usage` 字段（**不**使用 `flatten`，因为 usage 是嵌套对象）：

```rust
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}
```

#### 3.2 SSE 流式路径 — `StreamChunk` 扩展与最后一帧处理

**文件**：`crates/crabmate-types/src/chat_api.rs`

`StreamChunk` 增加 `usage` 字段（可选，仅在 SSE 最后一帧出现）：

```rust
#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Option<Vec<StreamChoice>>,
    /// SSE 最后一帧可能携带 usage（choices 为空时）。
    #[serde(default)]
    pub usage: Option<Usage>,
}
```

**文件**：`crates/crabmate-llm/src/api/sse_parser.rs`

当前 `ingest_sse_data_payload` 在 `choices` 为空时直接返回，usage 丢失。修正逻辑：

```rust
// 修改 ingest_sse_data_payload 函数（原第 548-553 行）
let chunk: StreamChunk = serde_json::from_slice(payload.as_bytes())
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

// 尝试提取 choices（可能为空）
if let Some(choices) = chunk.choices
    && let Some(choice) = choices.into_iter().next()
{
    ingest_sse_apply_finish_reason(finish_reason, &choice);
    let delta = choice.delta;
    // ... 现有 reasoning/content/tool_calls 处理 ...
}

// 提取 usage（可能在 choices 为空时携带）
if let Some(usage) = chunk.usage {
    // 通过回调或状态累积传递给调用方
    // 方案见下方 3.3
}
```

**提取方案**：`SseStreamAccum` 增加 `usage: Option<Usage>` 字段，流式解析完成后通过 `streaming_chat_response` 返回给调用方。

```rust
pub(super) struct SseStreamAccum {
    pub(super) reasoning_acc: String,
    pub(super) content_acc: String,
    pub(super) tool_calls_acc: Vec<(String, String, String, String)>,
    pub(super) finish_reason: String,
    pub(super) cli_plain_prefix_emitted: bool,
    pub(super) cli_plain_reasoning_style_active: bool,
    /// SSE 末尾帧携带的 usage（含缓存统计）。
    pub(super) usage: Option<Usage>,
}
```

#### 3.3 日志记录与 SSE 报告

**文件**：`crates/crabmate-llm/src/api/mod.rs`

在 `stream_chat` 返回后提取 usage 并记录。注意两条路径：

- **非流式**：`non_stream_chat_response` 中 `parsed.usage` 可直接获取
- **流式**：`streaming_chat_response` 中 `acc.usage` 从 SSE 末尾帧获取

统一提取函数：

```rust
/// 记录缓存命中统计。
fn log_cache_usage(usage: Option<&Usage>, model: &str) {
    let Some(u) = usage else { return };
    let hit = u.prompt_cache_hit_tokens.unwrap_or(0);
    let miss = u.prompt_cache_miss_tokens.unwrap_or(0);
    let total = hit + miss;
    let ratio = if total > 0 { hit as f64 / total as f64 } else { 0.0 };
    log::info!(
        target: "crabmate_llm",
        "prompt_cache model={} hit={} miss={} ratio={:.1}%",
        model, hit, miss, ratio * 100.0
    );
}
```

#### 3.4 进程级缓存统计累积

**文件**：`crates/crabmate-llm/src/retry.rs`（或独立的 `cache_stats.rs`）

使用 `std::sync::atomic::AtomicU64`（无需 `Mutex`，仅累加值）：

```rust
use std::sync::atomic::{AtomicU64, Ordering};

/// 进程级缓存累积统计（无锁原子操作）。
pub struct LlmCacheAggregate {
    total_hit_tokens: AtomicU64,
    total_miss_tokens: AtomicU64,
    request_count: AtomicU64,
}

impl LlmCacheAggregate {
    pub fn record(&self, usage: &Usage) {
        if let Some(hit) = usage.prompt_cache_hit_tokens {
            self.total_hit_tokens.fetch_add(hit, Ordering::Relaxed);
        }
        if let Some(miss) = usage.prompt_cache_miss_tokens {
            self.total_miss_tokens.fetch_add(miss, Ordering::Relaxed);
        }
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hit_ratio(&self) -> f64 {
        let hit = self.total_hit_tokens.load(Ordering::Relaxed);
        let miss = self.total_miss_tokens.load(Ordering::Relaxed);
        let total = hit + miss;
        if total == 0 { 0.0 } else { hit as f64 / total as f64 }
    }

    /// 估算节省金额（USD）。
    /// miss_rate = 未命中单价（如 V4-Flash $0.14/1M）
    /// hit_rate = 命中单价（如 $0.014/1M）
    pub fn estimated_savings(&self, miss_rate: f64, hit_rate: f64) -> f64 {
        let hit = self.total_hit_tokens.load(Ordering::Relaxed);
        hit as f64 * (miss_rate - hit_rate) / 1_000_000.0
    }
}
```

进程单例：

```rust
use std::sync::LazyLock;
pub static LLM_CACHE_AGGREGATE: LazyLock<LlmCacheAggregate> =
    LazyLock::new(|| LlmCacheAggregate::default());
```

在 `stream_chat` 返回后调用 `LLM_CACHE_AGGREGATE.record(usage)`。

#### 3.5 `diagnostic_summary` 集成

**文件**：`src/runtime/diagnostic_summary.rs`

```rust
/// LLM Prompt 缓存统计。
pub fn cache_stats() -> String {
    let agg = &*crate::llm::cache_stats::LLM_CACHE_AGGREGATE;
    format!(
        "LLM Prompt Cache: {} req, hit ratio {:.1}%, est. savings ${:.4}",
        agg.request_count(),
        agg.hit_ratio() * 100.0,
        agg.estimated_savings(0.14e-6, 0.014e-6)
    )
}
```

---

### Phase 1：System Prompt 稳定性优化（2-3 天）

#### 3.6 核心问题

DeepSeek 缓存从 **第 0 个 token 起** 开始匹配。Agent 的 system prompt 通常占 2000-5000 tokens，是最重要的缓存前缀。但当前 CrabMate 的 system prompt 构建路径中：

- `cursor_rules`、`skills`、`living_docs` 等动态内容累积到 system prompt
- 内容在轮次之间可能发生变化（文件被修改、cursor_rules 扩容等），导致前缀不一致

**不需要消息重排**：工具定义在 `ChatRequest.tools` 字段（不在 `messages` 中），DeepSeek V4 的公共前缀检测已能处理常见前缀变动。

#### 3.7 优化方向：system prompt 内容稳定性

**文件**：`crates/crabmate-llm/src/vendor_messages.rs`（system prompt 构建处）

```rust
/// 记录 system prompt 的 hash/长度，用于日志比对。
pub fn log_system_prompt_stability(messages: &[Message]) {
    let sys_len: usize = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_ref().map_or(0, |c| message_content_as_str(c).map_or(0, |s| s.len())))
        .sum();
    // 仅日志记录，不做截断
    log::info!(
        target: "crabmate_llm",
        "system_prompt_total_chars={}",
        sys_len
    );
}
```

**优化措施**：

| 措施 | 说明 | 影响 |
|------|------|------|
| System prompt hash 日志 | 每次调用前记录 system prompt 的 hash，运维可判断稳定性 | 零开销 |
| `diagnostic_summary` 新增 system prompt 变动次数 | 跨轮次计数 system prompt 哈希变化次数 | 零开销 |
| 配置项 `prompt_cache_stable_system` | 启用时尽量复用同一份 system prompt（减少动态注入） | 默认 `true` |

不改变代码逻辑，仅增加可观测性。

#### 3.8 配置项

**文件**：`crates/crabmate-config/src/types/agent_config_sections.rs`

```rust
pub struct LlmConnectionConfig {
    // ... 现有字段 ...
    /// 启用 prompt 缓存优化（当前仅影响日志可观测性）。
    pub prompt_cache_optimization: bool,
}
```

默认值：`true`

---

### Phase 2：显式 `cache_control`（3-5 天）

#### 3.9 DeepSeek `cache_control` 支持

DeepSeek 自 2026 年初支持 `cache_control` 参数（`type: "ephemeral"`，5 分钟窗口）[^3]。通过在 **system 消息**上标记 `cache_control` 可强制缓存该部分。

**注意**：`cache_control` 仅对 `role: "system"` 的消息有意义。`role: "tool"` 是工具执行结果（不断变化），不适合标记。

**供应商适配器扩展**：

```rust
pub trait LlmVendorAdapter: Send + Sync {
    // ... 现有方法 ...

    /// 供应商是否支持显式 cache_control 标记。
    fn supports_explicit_cache_control(&self) -> bool {
        false
    }
}
```

DeepSeek 适配器返回 `true`。

**注入位置**：在 `requests.rs` 的请求体构建函数中，在消息序列化**之前**注入 `cache_control`。

使用 `serde_json::Value` 扩展（不修改 `Message` 类型）：

```rust
/// 在 system 消息上注入 cache_control。
/// 通过 ChatRequestVendorExtensions 传递，不修改 Message 类型。
/// 实际方式：在发送前将 cache_control 作为消息的附加字段加入。
pub fn inject_cache_control_on_system(
    messages: &mut Vec<Message>,
    vendor: &dyn LlmVendorAdapter,
) {
    if !vendor.supports_explicit_cache_control() {
        return;
    }
    // 对第一条 system 消息注入 cache_control
    if let Some(msg) = messages.iter_mut().find(|m| m.role == "system") {
        // 利用 Message 已有的序列化能力，追加 cache_control 字段
        // 具体实现取决于 Message 的 JSON 表示
    }
}
```

**关于 `Message` 类型扩展的决策**：

`Message` 当前没有 `extensions`/`extra` 字段。有两种实现选择：

| 方案 | 做法 | 复杂度 |
|------|------|--------|
| A. 序列化时后处理 | 在 `ChatRequest` 序列化为 JSON 后手动插入 `cache_control` 到对应消息对象 | 低（不修改类型） |
| B. `Message` 增加 `extra_fields` | 在 `Message` 中新增 `#[serde(flatten)] extra: Option<HashMap<String, Value>>` | 中（影响所有序列化路径） |

**推荐方案 A**：请求体序列化后，在原始 JSON 的 system 消息对象中插入 `cache_control` 字段。这是最小侵入方案。

```rust
// 在 stream_chat 的序列化阶段（api/mod.rs 第 329 行）
// 现有：let mut rb = client.post(&url).json(&req);
// 改为序列化后注入 cache_control
let body = serde_json::to_value(&req)?;
let body = inject_cache_control_json(body, vendor);
let mut rb = client.post(&url).json(&body);
```

---

## 4. 类型变更汇总

| 类型/文件 | 变更 | Phase |
|-----------|------|-------|
| `Usage` (`chat_api.rs`) | **新增**结构体，含 `input_tokens` / `output_tokens` / `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` | 0 |
| `ChatResponse` (`chat_api.rs`) | 新增 `usage: Option<Usage>`（**非** flatten） | 0 |
| `StreamChunk` (`chat_api.rs`) | 新增 `usage: Option<Usage>` | 0 |
| `SseStreamAccum` (`sse_parser.rs`) | 新增 `usage: Option<Usage>` | 0 |
| `LlmCacheAggregate` （新文件/`retry.rs`） | 新增原子累积统计 + 进程单例 | 0 |
| `LlmConnectionConfig` | 新增 `prompt_cache_optimization: bool` | 1 |
| `LlmVendorAdapter` (`vendor/mod.rs`) | 新增 `supports_explicit_cache_control()` | 2 |

**`Message` 类型不变**。Phase 2 的 `cache_control` 通过 JSON 后处理实现，不修改核心类型。

---

## 5. 测试策略

| 测试类型 | 内容 | Phase |
|----------|------|-------|
| **单元测试** | `Usage` 反序列化（含缺失字段、无效值） | 0 |
| **单元测试** | `StreamChunk` 反序列化含 usage 字段 | 0 |
| **单元测试** | `LlmCacheAggregate` 原子累积正确性 | 0 |
| **单元测试** | `inject_cache_control_json` 仅作用于 system 消息 | 2 |
| **集成测试** | Mock HTTP 返回含 cache usage 的非流式响应 | 0 |
| **集成测试** | Mock SSE 流末尾帧含 usage，验证累积 | 0 |
| **端到端** | 真实 DeepSeek API 运行 3 轮调用，验证缓存命中 > 0 | 0 |

---

## 6. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 非 DeepSeek 供应商返回未知 `usage` 字段 | 低 | `#[serde(default)]` + `Option` 兜底 |
| SSE 末尾帧 usage 解析失败 | 低 | 现有 `serde_json::from_slice` 已稳健，失败时 usage 为 `None` |
| `cache_control` 被 API 忽略（无报错） | 低 | 仅在适配器声明支持时注入，不改变功能 |
| 原子累积丢失少量数据（并发写入） | 低 | `fetch_add` 是原子的，最多丢失一帧累积 |
| System prompt 内容变化无法强制稳定 | 中 | 仅可观测，不做内容截断——稳定性由上层系统 prompt 构建决定 |

---

## 7. 未来展望

### Phase 3（可选）
- **缓存预热**：系统启动时自动发送一次"热身"请求，提前缓存 system prompt
- **按会话的缓存命中率仪表盘**：在 Web UI 中显示实时缓存命中率

### 当 Anthropic 支持加入时
- `cache_control` breakpoints（多断点，不同 TTL）
- `prompt_caching` 独立计费字段

---

## 8. 参考资料

[^1]: DeepSeek API 上下文硬盘缓存：https://api-docs.deepseek.com/zh-cn/guides/kv_cache
[^2]: DeepSeek 缓存命中规则：前缀从第 0 个 token 起完全一致；V4 支持公共前缀检测
[^3]: DeepSeek `cache_control` 支持（2026 年初）：`ephemeral` 类型，5 分钟窗口
[^4]: V4-Flash 定价：常规 $0.14/1M，缓存命中 $0.014/1M（90% off）

---

## 附录 A：DeepSeek Response 示例

### 非流式响应

```json
{
  "id": "chatcmpl-xxx",
  "model": "deepseek-v4-flash",
  "usage": {
    "input_tokens": 1500,
    "output_tokens": 300,
    "prompt_cache_hit_tokens": 1200,
    "prompt_cache_miss_tokens": 300
  },
  "choices": [
    {
      "message": { "role": "assistant", "content": "Hello" },
      "finish_reason": "stop"
    }
  ]
}
```

### 流式响应末尾帧

```
data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":...,"model":"deepseek-v4-flash","choices":[],"usage":{"input_tokens":1500,"output_tokens":300,"prompt_cache_hit_tokens":1200,"prompt_cache_miss_tokens":300}}
data: [DONE]
```

注意 `choices: []` 且 `usage` 存在——这是当前 `StreamChunk` 解析丢失的分支。

## 附录 B：纠正的审校问题

| 版本 | 问题 | 修正 |
|------|------|------|
| v1 | `CacheUsage` 用 `#[serde(flatten)]` 直接放在 ChatResponse 顶层 | v2: 改为嵌套的 `Usage` 结构体，`usage: Option<Usage>` |
| v1 | 未覆盖流式路径 | v2: `StreamChunk` + `SseStreamAccum` 增加 `usage`，最后一帧提取 |
| v1 | `reorder_messages_for_cache` 消息重排（价值低） | v2: 移除，改为 system prompt 内容稳定性可观测 |
| v1 | `cache_control` 注入到 `role: "tool"`（语义混淆） | v2: 仅对 system 消息注入 |
| v1 | `Message.extensions` 修改核心类型 | v2: JSON 后处理方案，不修改 `Message` |
