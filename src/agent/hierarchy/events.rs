//! 分层多 Agent SSE 事件
//!
//! 提供分层执行过程中的可观测性事件
//! 使用现有的 ThinkingTrace 机制

use crate::sse::protocol::ThinkingTraceBody;

/// 分层执行阶段操作类型
const OP_HIERARCHICAL_STARTED: &str = "hierarchical_started";
const OP_HIERARCHICAL_FINISHED: &str = "hierarchical_finished";
const OP_MANAGER_STARTED: &str = "manager_started";
const OP_MANAGER_FINISHED: &str = "manager_finished";
const OP_LEVEL_STARTED: &str = "level_started";
const OP_LEVEL_FINISHED: &str = "level_finished";
const OP_SUBGOAL_STARTED: &str = "subgoal_started";
const OP_SUBGOAL_FINISHED: &str = "subgoal_finished";

/// 构建 Manager 开始分解任务的 ThinkingTrace
pub fn build_manager_started_trace(task: &str) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_MANAGER_STARTED.to_string(),
        node_id: None,
        parent_id: None,
        title: Some(format!("Manager 开始分解任务: {}", task)),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建 Manager 完成分解任务的 ThinkingTrace
pub fn build_manager_finished_trace(sub_goal_count: usize, strategy: &str) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_MANAGER_FINISHED.to_string(),
        node_id: None,
        parent_id: None,
        title: Some(format!(
            "Manager 完成分解: {} 个子目标, 策略={}",
            sub_goal_count, strategy
        )),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建层级开始执行的 ThinkingTrace
pub fn build_level_started_trace(level: usize, goal_ids: &[String]) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_LEVEL_STARTED.to_string(),
        node_id: Some(format!("level_{}", level)),
        parent_id: None,
        title: Some(format!(
            "开始执行第 {} 层, {} 个目标",
            level,
            goal_ids.len()
        )),
        chunk: Some(goal_ids.join(", ")),
        context_snapshot: None,
    }
}

/// 构建层级完成执行的 ThinkingTrace
pub fn build_level_finished_trace(
    level: usize,
    completed: usize,
    failed: usize,
) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_LEVEL_FINISHED.to_string(),
        node_id: Some(format!("level_{}", level)),
        parent_id: None,
        title: Some(format!(
            "第 {} 层完成: {} 成功, {} 失败",
            level, completed, failed
        )),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建子目标开始执行的 ThinkingTrace
pub fn build_subgoal_started_trace(goal_id: &str, description: &str) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_SUBGOAL_STARTED.to_string(),
        node_id: Some(goal_id.to_string()),
        parent_id: None,
        title: Some(format!("开始执行: {}", description)),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建子目标完成执行的 ThinkingTrace
pub fn build_subgoal_finished_trace(
    goal_id: &str,
    status: &str,
    duration_ms: u64,
) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_SUBGOAL_FINISHED.to_string(),
        node_id: Some(goal_id.to_string()),
        parent_id: None,
        title: Some(format!("子目标完成: {} ({}ms)", status, duration_ms)),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建分层执行开始的 ThinkingTrace
pub fn build_hierarchical_started_trace(
    total_sub_goals: usize,
    strategy: &str,
) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_HIERARCHICAL_STARTED.to_string(),
        node_id: None,
        parent_id: None,
        title: Some(format!(
            "分层执行开始: {} 个子目标, 策略={}",
            total_sub_goals, strategy
        )),
        chunk: None,
        context_snapshot: None,
    }
}

/// 构建分层执行完成的 ThinkingTrace
pub fn build_hierarchical_finished_trace(
    total_completed: usize,
    total_failed: usize,
    total_duration_ms: u64,
) -> ThinkingTraceBody {
    ThinkingTraceBody {
        op: OP_HIERARCHICAL_FINISHED.to_string(),
        node_id: None,
        parent_id: None,
        title: Some(format!(
            "分层执行完成: {} 成功, {} 失败 ({}ms)",
            total_completed, total_failed, total_duration_ms
        )),
        chunk: None,
        context_snapshot: None,
    }
}
