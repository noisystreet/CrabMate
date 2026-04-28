//! 分阶段规划在**定稿**后、**分步执行**前的回合级 orchestrator（见 `docs/design/per_state_machine_consolidation.md`）。
//! 仅负责 `staged_plan_started` 与首条队列摘要 `staged_plan_notice`；步级 SSE 仍在 `staged` 主循环内发送。

use tokio::sync::mpsc;

use crate::agent::plan_artifact::AgentReplyPlanV1;

use super::staged_sse::{
    send_staged_plan_notice, send_staged_plan_started, staged_plan_queue_summary_text,
};

/// 定稿后的规划已进入 **`StepsExecuting`**：已向 Web/终端发出 `staged_plan_started` 与首条 `notice`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedTurnPhase {
    StepsExecuting,
}

/// 发 `staged_plan_started` + 队列摘要 `staged_plan_notice`（`completed_count=0`）。
pub(crate) async fn enter_steps_executing(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    echo_terminal_staged: bool,
    plan_steps: &[crate::agent::plan_artifact::PlanStepV1],
) -> StagedTurnPhase {
    let n = plan_steps.len();
    send_staged_plan_started(out, plan_id, n).await;
    let plan_for_notice = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: plan_steps.to_vec(),
        no_task: false,
    };
    send_staged_plan_notice(
        out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan_for_notice, 0),
    )
    .await;
    StagedTurnPhase::StepsExecuting
}
