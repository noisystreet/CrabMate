//! 分阶段规划：首轮 JSON 解析成功后的**可选**第二轮无工具优化（合并探查步、提示单轮内批量并行工具）。

use std::collections::BTreeSet;

use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};
use crate::config::AgentConfig;
use crate::types::Tool;

/// 步骤优化轮注入的 user 正文标记（取消/失败时弹出临时 user）。
pub(crate) const STAGED_PLAN_OPTIMIZER_COACH_MARK: &str = "### 分阶段规划 · 步骤优化（服务端注入）";

/// 本会话 `tools_defs` 中，满足「同轮可多调用并行 `spawn_blocking`」的工具名（逗号分隔，已排序去重）。
pub(crate) fn parallel_batchable_tool_names_csv_from_defs(
    tools: &[Tool],
    cfg: &AgentConfig,
) -> String {
    let mut names = BTreeSet::new();
    for t in tools {
        let n = t.function.name.as_str();
        if crate::tool_registry::tool_ok_for_parallel_readonly_batch_piece(cfg, n) {
            names.insert(n.to_string());
        }
    }
    names.into_iter().collect::<Vec<_>>().join(", ")
}

/// 生成「规划优化轮」注入的 user 正文（中文）。
pub(crate) fn staged_plan_optimizer_user_body(
    plan: &AgentReplyPlanV1,
    parallel_tool_names_csv: &str,
) -> String {
    let steps_md = plan_artifact::format_plan_steps_markdown(plan);
    let batch_tools_line = if parallel_tool_names_csv.trim().is_empty() {
        "- 不仅限于 `read_file`：凡可在**同一助手轮**内并行发起的**只读**内建工具，应优先放在**同一步**中批量使用。当前会话工具列表中**暂无**经服务端判定为「可同轮并行批处理」的内建名；仍请尽量合并无依赖的只读探查步。"
            .to_string()
    } else {
        format!(
            "- 不仅限于 `read_file`：凡可在**同一助手轮**内并行发起的只读内建工具，应优先放在**同一步**中批量使用。本进程当前会话下可同轮并行批处理的内建工具名包括：{}。",
            parallel_tool_names_csv
        )
    };
    format!(
        "{}\n\
         下面是你刚输出的可执行规划（Markdown 列表，**勿**改动其中 `id`，除非合并/拆分时为新步分配新 id）。\n\
         请在**不削弱任务覆盖面**的前提下优化 `steps`：\n\
         - 将**连续、彼此无数据依赖**且主要为**只读探查**的子目标合并为更少的大步（一步内可安排**多次**工具调用）。\n\
         {batch_tools_line}\n\
         - **不要**把「必须先读后写」或「必须先分析再改」的强依赖硬塞进同一步；写盘、`run_command`、网络变更类步骤保持独立或放在只读探查之后。\n\
         - 若原规划已足够紧凑，可原样返回（`steps` 与下列列表等价即可）。\n\n\
         **输出要求**：仅输出一段可解析的 `agent_reply_plan` v1 JSON（可用 ```json 围栏），`type`/`version`/`steps` 与既有 schema 一致；`no_task` 须为 false 且 `steps` 非空。\n\n\
         Schema：{}\n\
         示例：\n```json\n{}\n```\n\n\
         ---\n\
         当前规划步骤：\n\
         {}",
        STAGED_PLAN_OPTIMIZER_COACH_MARK,
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON,
        steps_md,
    )
}

/// 若优化轮解析成功且 `steps` 非空，返回新步骤；否则返回 `None`（调用方沿用首轮规划）。
pub(crate) fn try_parse_optimizer_reply(content: &str) -> Option<Vec<PlanStepV1>> {
    let p = plan_artifact::parse_agent_reply_plan_v1(content).ok()?;
    if p.no_task || p.steps.is_empty() {
        return None;
    }
    Some(p.steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDef, Tool};

    #[test]
    fn parallel_csv_from_defs_filters_by_registry() {
        let tools = vec![
            Tool {
                typ: "function".to_string(),
                function: FunctionDef {
                    name: "read_file".to_string(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
            Tool {
                typ: "function".to_string(),
                function: FunctionDef {
                    name: "create_file".to_string(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let cfg = crate::config::load_config(None).expect("embed default");
        let csv = parallel_batchable_tool_names_csv_from_defs(&tools, &cfg);
        assert!(csv.contains("read_file"));
        assert!(!csv.contains("create_file"));
    }

    #[test]
    fn try_parse_optimizer_rejects_no_task() {
        let body = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        assert!(try_parse_optimizer_reply(body).is_none());
    }
}
