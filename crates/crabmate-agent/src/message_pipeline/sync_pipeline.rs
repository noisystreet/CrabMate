//! 会话同步管道编排与可观测性（阶段枚举、报告、[`apply_session_sync_pipeline_with_config`]）。

use std::sync::atomic::Ordering;

use crabmate_config::AgentConfig;
use crabmate_types::Message;

use super::transforms::{
    compress_tool_message_contents, drop_orphan_tool_messages, estimate_non_system_chars,
    trim_messages_by_char_budget, trim_messages_by_count,
};
use super::{MESSAGE_PIPELINE_COUNTERS, MessagePipelineConfig, MessagePipelineCounters};

/// 会话同步管道中「一步」的标签（顺序即 [`apply_session_sync_pipeline_with_config`] 中的执行顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagePipelineStage {
    /// 管道入口（尚未改动 `messages`）。
    SessionSyncStart,
    AfterCompressTool,
    AfterTrimByCount,
    AfterTrimByCharBudget,
    AfterSecondCompressTool,
    AfterDropOrphanTool,
    AfterMergeAssistantsInPlace,
}

impl MessagePipelineStage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SessionSyncStart => "start",
            Self::AfterCompressTool => "after_compress_tool",
            Self::AfterTrimByCount => "after_trim_count",
            Self::AfterTrimByCharBudget => "after_trim_char_budget",
            Self::AfterSecondCompressTool => "after_compress_tool_2",
            Self::AfterDropOrphanTool => "after_drop_orphan_tool",
            Self::AfterMergeAssistantsInPlace => "after_merge_assistants",
        }
    }
}

/// 一步后的消息列表度量（供 debug 日志）。
#[derive(Clone, Debug)]
pub struct PipelineStepSnapshot {
    pub stage: MessagePipelineStage,
    pub message_count: usize,
    pub non_system_chars_est: usize,
}

/// 会话同步管道的一次执行轨迹（仅 debug 启用时填充）。
#[derive(Default, Debug)]
pub struct MessagePipelineReport {
    pub steps: Vec<PipelineStepSnapshot>,
}

impl MessagePipelineReport {
    fn record(&mut self, stage: MessagePipelineStage, messages: &[Message]) {
        self.steps.push(PipelineStepSnapshot {
            stage,
            message_count: messages.len(),
            non_system_chars_est: estimate_non_system_chars(messages),
        });
    }

    /// 单行摘要，便于 `log` debug 汇总。
    pub fn format_for_log(&self) -> String {
        let parts: Vec<String> = self
            .steps
            .iter()
            .map(|s| {
                format!(
                    "{}:n={}:chars≈{}",
                    s.stage.as_str(),
                    s.message_count,
                    s.non_system_chars_est
                )
            })
            .collect();
        parts.join(" | ")
    }
}

fn log_session_sync_step(stage: MessagePipelineStage, messages: &[Message]) {
    log::trace!(
        target: "crabmate::message_pipeline",
        "session_sync_step stage={} message_count={} non_system_chars_est={}",
        stage.as_str(),
        messages.len(),
        estimate_non_system_chars(messages),
    );
}

fn record_and_trace(
    report: &mut Option<&mut MessagePipelineReport>,
    stage: MessagePipelineStage,
    messages: &[Message],
) {
    if let Some(r) = report.as_mut() {
        r.record(stage, messages);
    }
    log_session_sync_step(stage, messages);
}

/// 每次调用模型前对会话 `messages` 的**同步**变换；配置为 [`MessagePipelineConfig`]（单测可构造轻量配置）。
///
/// `report` 为 `Some` 时写入各步快照（`context_window` 在 debug 时传入）；**任意** `RUST_LOG` 含 **`crabmate::message_pipeline=trace`** 时每步另打结构化 trace。
pub fn apply_session_sync_pipeline_with_config(
    messages: &mut Vec<Message>,
    cfg: MessagePipelineConfig,
    mut report: Option<&mut MessagePipelineReport>,
) {
    let ctr: &MessagePipelineCounters = &MESSAGE_PIPELINE_COUNTERS;

    record_and_trace(
        &mut report,
        MessagePipelineStage::SessionSyncStart,
        messages,
    );

    let c1 = compress_tool_message_contents(messages, cfg.tool_message_max_chars);
    if c1 > 0 {
        ctr.tool_compress_hits
            .fetch_add(c1 as u64, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterCompressTool,
        messages,
    );

    if trim_messages_by_count(messages, cfg.max_message_history) {
        ctr.trim_count_hits.fetch_add(1, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterTrimByCount,
        messages,
    );

    if cfg.context_char_budget > 0 {
        if trim_messages_by_char_budget(
            messages,
            cfg.context_char_budget,
            cfg.context_min_messages_after_system,
        ) {
            ctr.trim_char_budget_hits.fetch_add(1, Ordering::Relaxed);
        }
        record_and_trace(
            &mut report,
            MessagePipelineStage::AfterTrimByCharBudget,
            messages,
        );
        let c2 = compress_tool_message_contents(messages, cfg.tool_message_max_chars);
        if c2 > 0 {
            ctr.tool_compress_hits
                .fetch_add(c2 as u64, Ordering::Relaxed);
        }
        record_and_trace(
            &mut report,
            MessagePipelineStage::AfterSecondCompressTool,
            messages,
        );
    }

    let dropped = drop_orphan_tool_messages(messages);
    if dropped > 0 {
        ctr.orphan_tool_drops
            .fetch_add(dropped as u64, Ordering::Relaxed);
    }
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterDropOrphanTool,
        messages,
    );

    crabmate_types::merge_consecutive_assistants_in_place(messages);
    record_and_trace(
        &mut report,
        MessagePipelineStage::AfterMergeAssistantsInPlace,
        messages,
    );
}

/// 与 [`apply_session_sync_pipeline_with_config`] 相同，从完整 [`AgentConfig`] 取子集。
pub fn apply_session_sync_pipeline(
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
    report: Option<&mut MessagePipelineReport>,
) {
    apply_session_sync_pipeline_with_config(messages, MessagePipelineConfig::from(cfg), report);
}
