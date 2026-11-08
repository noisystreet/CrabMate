//! 单条 Web `/chat` 任务在跑 `run_agent_turn` 时 PER 相关状态的只读镜像（进程内、按 `job_id` 区分）。
//!
//! 类型与 [`crate::chat_job_queue`] 解耦，便于在未启用 `web` feature 时仍被 Agent 编排层引用（CLI 通常为 `None`）。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
}

impl PerTurnFlight {
    pub fn sync_from_per_coord(&self, p: &crate::agent::per_coord::PerCoordinator) {
        self.plan_rewrite_attempts
            .store(p.plan_rewrite_attempts_snapshot(), Ordering::Relaxed);
        self.require_plan_in_final_content
            .store(p.require_plan_in_final_flag_snapshot(), Ordering::Relaxed);
    }
}
