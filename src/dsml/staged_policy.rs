//! 分阶段无工具规划轮：检测正文 DSML 违规（物化后清空，不执行工具）。

use log::{debug, warn};

use crate::types::Message;

use super::adapter::DsmlToolCallAdapter;
use super::types::{DsmlMaterializePolicy, StagedDsmlHandling, StagedDsmlScanResult};

fn log_discard_native_tool_calls(round_hint: &str, count: usize) {
    debug!(
        target: "crabmate",
        "分阶段规划{round_hint}：丢弃 API 返回的 {count} 条原生 tool_calls，改从正文解析"
    );
}

fn log_discard_materialized_tool_calls(round_hint: &str, count: usize) {
    warn!(
        target: "crabmate",
        "分阶段规划{round_hint}：正文物化出 {count} 条 tool_calls，已忽略，仅尝试从正文解析规划 JSON"
    );
}

fn scan_after_staged_handling(
    msg: &mut Message,
    handling: StagedDsmlHandling,
    materialize_enabled: bool,
    round_hint: &str,
) -> StagedDsmlScanResult {
    let policy = DsmlMaterializePolicy::from_enabled(materialize_enabled);
    let raw_native_count = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    if raw_native_count > 0 {
        log_discard_native_tool_calls(round_hint, raw_native_count);
    }
    msg.tool_calls = None;
    DsmlToolCallAdapter::new().apply_to_assistant_message(msg, policy);
    let materialized_count = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0);
    if materialized_count > 0 && matches!(handling, StagedDsmlHandling::DetectOnly) {
        log_discard_materialized_tool_calls(round_hint, materialized_count);
    }
    msg.tool_calls = None;
    StagedDsmlScanResult {
        raw_native_count,
        materialized_count,
    }
}

/// 与首轮/优化轮一致：忽略原生 tool_calls，物化 DSML 后再清空，仅解析正文规划 JSON。
pub(crate) fn strip_staged_planner_message_tool_calls(
    msg: &mut Message,
    round_hint: &'static str,
    materialize_enabled: bool,
) {
    scan_after_staged_handling(
        msg,
        StagedDsmlHandling::DetectOnly,
        materialize_enabled,
        round_hint,
    );
}

/// 首轮规划 assistant：清空原生 tool_calls 后经 DSML 物化，返回等价 tool_calls 条数总和。
pub(crate) fn staged_first_planner_tool_call_total_after_materialize(
    msg: &mut Message,
    materialize_enabled: bool,
) -> usize {
    scan_after_staged_handling(
        msg,
        StagedDsmlHandling::CountForRewrite,
        materialize_enabled,
        "·首轮",
    )
    .total_for_rewrite_trigger()
}

/// 自然语言补全轮 / 补丁轮等：检测 DSML 违规并清空 `tool_calls`。
pub(crate) fn staged_no_tools_scan(
    msg: &mut Message,
    materialize_enabled: bool,
    round_hint: &str,
) -> StagedDsmlScanResult {
    scan_after_staged_handling(
        msg,
        StagedDsmlHandling::DetectOnly,
        materialize_enabled,
        round_hint,
    )
}

/// 返回物化出的 tool_calls 条数（调用方负责 timeline / 早退）。
pub(crate) fn staged_no_tools_materialized_count(
    msg: &mut Message,
    materialize_enabled: bool,
    round_hint: &str,
) -> usize {
    staged_no_tools_scan(msg, materialize_enabled, round_hint).materialized_count
}
