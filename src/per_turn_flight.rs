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
    /// 本 `run_agent_turn` 内已成功合并的分阶段补丁规划轮次数（与 `plan_rewrite_attempts` 独立）。
    pub staged_plan_patch_planner_rounds_completed: AtomicUsize,
    /// 配置镜像：`staged_plan_patch_max_attempts`（供 `/status` 与排障对照）。
    pub staged_plan_patch_max_attempts_config: AtomicUsize,
    pub require_plan_in_final_content: AtomicBool,
}

impl PerTurnFlight {
    pub fn sync_from_per_coord(&self, p: &crate::agent::per_coord::PerCoordinator) {
        self.plan_rewrite_attempts
            .store(p.plan_rewrite_attempts_snapshot(), Ordering::Relaxed);
        self.staged_plan_patch_planner_rounds_completed.store(
            p.staged_plan_patch_planner_rounds_snapshot(),
            Ordering::Relaxed,
        );
        self.staged_plan_patch_max_attempts_config.store(
            p.staged_plan_patch_max_attempts_config_snapshot(),
            Ordering::Relaxed,
        );
        self.require_plan_in_final_content
            .store(p.require_plan_in_final_flag_snapshot(), Ordering::Relaxed);
    }
}
