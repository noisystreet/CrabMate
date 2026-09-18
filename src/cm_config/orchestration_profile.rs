//! 编排路径摘要：运行时恒为 ReAct（单 Agent 外循环），不再有档位配置。

/// 本进程有效编排路径摘要（`doctor` / `GET /status`）。
pub fn effective_orchestration_path_summary() -> String {
    // 运行时只有 ReAct 外循环（单 agent 模式已固定）。
    "react outer loop".to_string()
}
