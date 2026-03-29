//! 分阶段规划：可选「逻辑多规划员」——首轮后再跑 1～2 轮独立无工具规划，最后合并为单一 `steps`。
//!
//! 仍为**同进程、同模型、串行 API**：通过不同 system 侧 user 注入模拟多角色；不保证更优，且 **API 次数与费用**随 `staged_plan_ensemble_count` 增加。

use crate::agent::plan_artifact::{self, AgentReplyPlanV1, PlanStepV1};

/// 规划员 B 的 user 注入标记（用于取消/失败时弹出临时 user，避免孤立上下文）。
pub(crate) const STAGED_PLAN_ENSEMBLE_B_COACH_MARK: &str =
    "### 分阶段规划 · 逻辑规划员 B（服务端注入）";

/// 规划员 C 的 user 注入标记。
pub(crate) const STAGED_PLAN_ENSEMBLE_C_COACH_MARK: &str =
    "### 分阶段规划 · 逻辑规划员 C（服务端注入）";

/// 合并轮的 user 注入标记。
pub(crate) const STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK: &str =
    "### 分阶段规划 · 合并多规划（服务端注入）";

/// 是否为逻辑多规划员流程中注入的临时 user（取消或解析失败时弹出）。
pub(crate) fn is_ensemble_injected_user_content(content: &str) -> bool {
    content.contains(STAGED_PLAN_ENSEMBLE_B_COACH_MARK)
        || content.contains(STAGED_PLAN_ENSEMBLE_C_COACH_MARK)
        || content.contains(STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK)
}

fn format_planner_label(idx: usize) -> String {
    let name = match idx {
        1 => "规划员 A（首轮）",
        2 => "规划员 B",
        3 => "规划员 C",
        _ => return format!("规划员 {}", idx),
    };
    name.to_string()
}

fn format_prior_plans_markdown(plans: &[AgentReplyPlanV1]) -> String {
    let mut blocks = String::new();
    for (i, p) in plans.iter().enumerate() {
        let label = format_planner_label(i + 1);
        let md = plan_artifact::format_plan_steps_markdown(p);
        blocks.push_str(&format!(
            "#### {}\n{}\n\n",
            label,
            if md.trim().is_empty() {
                "（无步骤）".to_string()
            } else {
                md
            }
        ));
    }
    blocks.trim_end().to_string()
}

/// 追加规划员 B/C 时注入的 user 正文（`prior` 为已接受的规划，不含本轮）。
pub(crate) fn ensemble_secondary_planner_user_body(
    planner_index: u8,
    prior: &[AgentReplyPlanV1],
) -> String {
    let priors = format_prior_plans_markdown(prior);
    let role = match planner_index {
        2 => "规划员 B",
        3 => "规划员 C",
        _ => "追加规划员",
    };
    let coach = match planner_index {
        2 => STAGED_PLAN_ENSEMBLE_B_COACH_MARK,
        3 => STAGED_PLAN_ENSEMBLE_C_COACH_MARK,
        _ => "### 分阶段规划 · 追加规划员（服务端注入）",
    };
    format!(
        "{coach}\n\
         你是**{role}**：与上文用户诉求相同，但须**独立**重新分解任务，输出一份完整的 `agent_reply_plan` v1 JSON。\n\
         下方仅为其他规划员的方案，**可对齐、可反对**；若发现遗漏、顺序不当或可合并的只读探查，请在**你的**规划中修正。\n\
         **输出要求**：仅输出可解析的 v1 JSON（可用 ```json 围栏），`type`/`version`/`steps` 符合 schema；`no_task` 须为 false 且 `steps` 非空（与首轮一致：有具体可执行子目标时不得空）。\n\n\
         Schema：{}\n\
         示例：\n```json\n{}\n```\n\n\
         ---\n\
         其他规划员方案（对照用）：\n\
         {priors}",
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON,
    )
}

/// 合并多份规划为单一 `agent_reply_plan` v1 的 user 正文。
pub(crate) fn ensemble_merge_planner_user_body(plans: &[AgentReplyPlanV1]) -> String {
    let n = plans.len();
    let body_plans = format_prior_plans_markdown(plans);
    format!(
        "{}\n\
         以下共 **{n}** 份由不同逻辑规划员给出的步骤列表（同一用户任务）。请**综合**为**一份**最优执行计划。\n\
         - 覆盖全部必要子目标，去掉明显重复；理清依赖：**先读后写**、**先分析后改**。\n\
         - 可合并彼此无依赖的只读探查到更少步（一步内可多次工具调用）。\n\
         - 为每步分配**新的**非空 `id`（可带合理前缀如 `m1-`）。\n\
         **输出要求**：仅输出一段 `agent_reply_plan` v1 JSON（可用 ```json 围栏），`no_task` 须为 false，`steps` 非空。\n\n\
         Schema：{}\n\
         示例：\n```json\n{}\n```\n\n\
         ---\n\
         {body_plans}",
        STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK,
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON,
    )
}

/// 解析追加规划员或合并轮回复；`no_task` 或空 `steps` 视为失败。
pub(crate) fn try_parse_ensemble_planner_reply(content: &str) -> Option<Vec<PlanStepV1>> {
    let p = plan_artifact::parse_agent_reply_plan_v1(content).ok()?;
    if p.no_task || p.steps.is_empty() {
        return None;
    }
    Some(p.steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;

    #[test]
    fn secondary_body_contains_coach_b() {
        let prior = vec![AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "a1".to_string(),
                description: "x".to_string(),
                workflow_node_id: None,
            }],
            no_task: false,
        }];
        let s = ensemble_secondary_planner_user_body(2, &prior);
        assert!(s.contains(STAGED_PLAN_ENSEMBLE_B_COACH_MARK));
        assert!(s.contains("规划员 A"));
    }

    #[test]
    fn merge_body_lists_two_planners() {
        let p1 = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "1".to_string(),
                description: "d1".to_string(),
                workflow_node_id: None,
            }],
            no_task: false,
        };
        let p2 = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "2".to_string(),
                description: "d2".to_string(),
                workflow_node_id: None,
            }],
            no_task: false,
        };
        let s = ensemble_merge_planner_user_body(&[p1, p2]);
        assert!(s.contains(STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK));
        assert!(s.contains("规划员 A"));
        assert!(s.contains("规划员 B"));
    }
}
