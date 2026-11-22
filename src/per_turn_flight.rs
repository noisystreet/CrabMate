//! 单条 Web `/chat` 任务在跑 `run_agent_turn` 时 PER 相关状态的只读镜像（进程内、按 `job_id` 区分）。
//!
//! 类型与 [`crate::chat_job_queue`] 解耦，便于在未启用 `web` feature 时仍被 Agent 编排层引用（CLI 通常为 `None`）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::cm_agent::completion_judge::CompletionVerdict;

/// 单条 `/chat` / `/chat/stream` 任务在跑 `run_agent_turn` 时，PER 相关状态的只读镜像（进程内、按 `job_id` 区分）。
///
/// **局限**：与浏览器「会话」无稳定绑定；同一客户端连续请求会得到不同 `job_id`。完整「本会话是否在规划重写」需会话级协议（如 `conversation_id`）再关联。
#[derive(Debug, Default)]
pub struct PerTurnFlight {
    /// 已追加「请重写终答规划」的 user 消息，正在等待下一轮模型输出。
    pub awaiting_plan_rewrite_model: AtomicBool,
    pub plan_rewrite_attempts: AtomicUsize,
    pub require_plan_in_final_content: AtomicBool,
    /// 层 1：本轮最后一次 assistant 输出的上游 `finish_reason` 是否表示「输出被截断」。
    /// 供编排层在 `stream_ended` 终态里区分「截断」与「正常完成 / 无输出」。
    pub last_assistant_finish_truncated: AtomicBool,
    /// 本回合是否已下发 `timeline_log kind=final_response`（终答标记）。
    ///
    /// **谁发谁置位**：当前唯一的生产者是 `chat_job_queue::stream_finish` 的层 4 兜底，
    /// 它用 `swap(true)` 兼作幂等闸；将来若改由 Agent 主链路直接下发，也必须在这里置位，
    /// 否则兜底会重复补发（客户端会看到两条终答时间线）。见 #1316。
    pub final_response_emitted: AtomicBool,
    /// 本回合完成判定第 ② 层（LLM）的**结构化结论**（#1472）。
    ///
    /// 与 `last_assistant_finish_truncated` 同理：判定发生在 agent 编排层，消费方在
    /// `chat_job_queue::stream_finish`，两者只通过本结构通信。
    /// `None` = 「本回合没有可用结论」（judge 未跑 / 开关关 / 失败 / 证据已变），
    /// 消费方**必须 fail-open**（按 `completed` 处理）。
    pub completion_verdict: Mutex<Option<CompletionVerdict>>,
}

impl PerTurnFlight {
    pub fn sync_from_per_coord(&self, p: &crate::agent::per_coord::PerCoordinator) {
        self.plan_rewrite_attempts
            .store(p.plan_rewrite_attempts_snapshot(), Ordering::Relaxed);
        self.require_plan_in_final_content
            .store(p.require_plan_in_final_flag_snapshot(), Ordering::Relaxed);
    }

    /// 记录本回合的完成判定结论（由 `run_agent_turn_common` 收尾处写入）。
    pub fn set_completion_verdict(&self, verdict: Option<CompletionVerdict>) {
        if let Ok(mut slot) = self.completion_verdict.lock() {
            *slot = verdict;
        }
    }

    /// 读取本回合的完成判定结论（供 `stream_finish` 决定 `stream_ended.reason`）。
    #[must_use]
    pub fn completion_verdict(&self) -> Option<CompletionVerdict> {
        self.completion_verdict.lock().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::PerTurnFlight;
    use crate::cm_agent::completion_judge::{CompletionStatus, CompletionVerdict};

    /// #1472：结论从「agent 编排层」搬到「`stream_finish` 消费方」只经由本结构，
    /// 所以要保证读写往返原样、且能被显式清空（fail-open 路径）。
    ///
    /// **覆盖边界**：这里只验证通道本身。`judge → PerCoordinator → PerTurnFlight` 的
    /// 端到端搬运需要给 `turn_early_stop_allowed_with_judge` 注入 mock judge，
    /// 现有 mock 集成测（`tests/run_agent_turn_orchestration_mock.rs`）硬关 judge 且
    /// `per_flight: None`，接不进来——属 follow-up，不在本提交内假装覆盖。
    #[test]
    fn completion_verdict_roundtrips() {
        let flight = PerTurnFlight::default();
        assert!(flight.completion_verdict().is_none(), "默认应为无结论");

        let partial = CompletionVerdict::new(
            CompletionStatus::Partial,
            Vec::new(),
            vec!["TUI 仍编译不过".to_string()],
        );
        flight.set_completion_verdict(Some(partial.clone()));
        assert_eq!(
            flight.completion_verdict(),
            Some(partial),
            "写入后应原样读回（含 missing）"
        );

        flight.set_completion_verdict(None);
        assert!(flight.completion_verdict().is_none(), "可被清空");
    }
}
