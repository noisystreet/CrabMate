//! 对话 **`Message` 变换管道**：统一「会话内存侧」与「发往供应商」两阶段的处理顺序与可观测性。
//!
//! ## 两阶段
//!
//! 1. **会话同步（`apply_session_sync_pipeline`）**：在每次调用模型前对**进程内** `Vec<Message>` 就地处理——工具正文压缩（`crabmate_tool` 信封内 **`output`** 超长时首尾采样 + 元数据，见 [`crabmate_internal::tool_result::maybe_compress_tool_message_content`]）、条数/字符裁剪、孤立 `tool` 剔除、合并相邻 `assistant`（保留会话尾部空占位语义，见 [`crabmate_types::normalize_messages_for_openai_compatible_request`] 文档）。实现原在 [`super::context_window`]，现经本模块编排。
//! 2. **供应商出站（`conversation_messages_to_vendor_body` 等）**：从会话切片构造 **`ChatRequest.messages`**：跳过 UI 分隔线与长期记忆注入、按网关策略去掉 `reasoning_content`（Moonshot **kimi-k2.5** 在 thinking 启用时对含 **`tool_calls`** 的 assistant **保留**思维链，见 [`crate::llm::vendor::LlmVendorAdapter::preserve_assistant_tool_call_reasoning`]）、再经 OpenAI 兼容 normalize（合并相邻 assistant、清理尾部非法 assistant）；若调用方传入的 **`fold_system_into_user`** 为真（由 [`crate::llm::fold_system_into_user_for_config`] 按 MiniMax 等网关判定），再将 **`system`** 折叠进后续 **`user`**。**不**写入会话 `Vec`。
//!
//! ## 会话同步顺序契约（勿打乱）
//!
//! 对 [`apply_session_sync_pipeline`] / [`apply_session_sync_pipeline_with_config`] 中**固定**为：
//!
//! 1. 记录起点快照（`SessionSyncStart`）
//! 2. **`compress_tool_message_contents`**（`tool_message_max_chars`）
//! 3. **`trim_messages_by_count`**（`max_message_history`）
//! 4. 若 **`context_char_budget > 0`**：`trim_messages_by_char_budget` → 再次 **`compress_tool_message_contents`**
//! 5. **`drop_orphan_tool_messages`**
//! 6. **`merge_consecutive_assistants_in_place`**
//!
//! 新增步骤须同步更新 [`sync_pipeline::MessagePipelineStage`]、本列表、以及 `docs/开发文档.md` 中上下文策略描述。
//!
//! ## 可观测性
//!
//! - **Debug**：`context_window` 在 `RUST_LOG` 含 debug 时打一行汇总（`message_pipeline session_sync: …`）。
//! - **Trace**：`target=crabmate::message_pipeline`，每步一行 `session_sync_step stage=… message_count=… non_system_chars_est=…`（便于 grep/采集）；设置 `RUST_LOG=crabmate::message_pipeline=trace`。
//!
//! 新增处理步骤时：优先在 **`sync_pipeline.rs`** 增加 `MessagePipelineStage` 变体，并在 `apply_session_sync_pipeline_with_config` 中按固定顺序调用，避免在 `agent_turn` / `llm` 多处散落。
//!
//! ## 子模块
//!
//! - **`transforms`**：条数/字符裁剪、tool 压缩、孤立 tool 剔除等逐步变换。
//! - **`sync_pipeline`**：阶段枚举、报告与 `apply_session_sync_pipeline*` 编排。
//! - **`vendor`**（再导出）：[`conversation_messages_to_vendor_body`] 等出站路径（实现于 **`crabmate_llm::vendor_messages`**）。

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crabmate_config::AgentConfig;

mod sync_pipeline;
mod transforms;

#[cfg(test)]
mod tests;

pub use crabmate_llm::vendor_messages::{
    conversation_messages_to_vendor_body, normalize_stripped_messages_for_vendor_body,
};
pub use sync_pipeline::{
    MessagePipelineReport, MessagePipelineStage, PipelineStepSnapshot, apply_session_sync_pipeline,
    apply_session_sync_pipeline_with_config,
};
pub use transforms::{
    compress_tool_message_contents, drop_orphan_tool_messages, estimate_message_chars,
    estimate_non_system_chars, trim_messages_by_char_budget, trim_messages_by_count,
};

/// 进程内累计：每次 `prepare_messages_for_model` 内同步管道实际发生裁剪/剔除时递增（供 `GET /status` 排障）。
#[derive(Debug, Default)]
pub struct MessagePipelineCounters {
    pub trim_count_hits: AtomicU64,
    pub trim_char_budget_hits: AtomicU64,
    pub tool_compress_hits: AtomicU64,
    pub orphan_tool_drops: AtomicU64,
}

impl MessagePipelineCounters {
    pub fn snapshot(&self) -> MessagePipelineCountersSnapshot {
        MessagePipelineCountersSnapshot {
            trim_count_hits: self.trim_count_hits.load(Ordering::Relaxed),
            trim_char_budget_hits: self.trim_char_budget_hits.load(Ordering::Relaxed),
            tool_compress_hits: self.tool_compress_hits.load(Ordering::Relaxed),
            orphan_tool_drops: self.orphan_tool_drops.load(Ordering::Relaxed),
        }
    }
}

/// `MessagePipelineCounters::snapshot()` 的纯数据副本（可序列化到 `/status`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct MessagePipelineCountersSnapshot {
    pub trim_count_hits: u64,
    pub trim_char_budget_hits: u64,
    pub tool_compress_hits: u64,
    pub orphan_tool_drops: u64,
}

/// 全局计数器（Web 与 CLI 共用同一进程内实例）。
pub static MESSAGE_PIPELINE_COUNTERS: LazyLock<MessagePipelineCounters> =
    LazyLock::new(MessagePipelineCounters::default);

/// 会话同步管道所用配置子集（便于测试与不依赖完整 [`AgentConfig`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagePipelineConfig {
    pub tool_message_max_chars: usize,
    pub max_message_history: usize,
    pub context_char_budget: usize,
    pub context_min_messages_after_system: usize,
}

impl From<&AgentConfig> for MessagePipelineConfig {
    fn from(cfg: &AgentConfig) -> Self {
        Self {
            tool_message_max_chars: cfg.tool_transcript.tool_message_max_chars,
            max_message_history: cfg.session_ui.max_message_history,
            context_char_budget: cfg.effective_context_char_budget_for_pipeline(),
            context_min_messages_after_system: cfg
                .context_pipeline
                .context_min_messages_after_system,
        }
    }
}
