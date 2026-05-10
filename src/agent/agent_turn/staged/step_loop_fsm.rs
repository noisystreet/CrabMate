//! 分阶段 **`run_staged_plan_steps_loop`** 内大块编排的纯决策：**步骤 transitions 跳转**与注入 **user 正文**。
//! **不**运行 outer_loop / 不发 SSE；调用方负责 I/O。

use std::collections::HashMap;

use crate::agent::plan_artifact::{PlanStepControlFlow, PlanStepV1};

/// 与 `mod.rs` 原 `compute_transition_trigger` 等价：匹配一条 transition、遵守 `max_loops`、更新计数器。
pub(crate) fn staged_step_transition_trigger(
    step: &PlanStepV1,
    run_failed_or_verify_failed: bool,
    step_verify_failed_reason: &Option<String>,
    transition_counters: &mut HashMap<String, u32>,
) -> Option<(String, String)> {
    let transitions = step.transitions.as_ref()?;
    let target = select_transition_rule(transitions, run_failed_or_verify_failed)?;
    let key = format!("{}->{}", step.id, target.target_step_id);
    let count = transition_counters.entry(key).or_insert(0);
    if *count >= target.max_loops.unwrap_or(3) {
        return None;
    }
    *count += 1;
    let reason = if run_failed_or_verify_failed {
        step_verify_failed_reason
            .clone()
            .unwrap_or_else(|| "执行错误".to_string())
    } else {
        "执行成功".to_string()
    };
    Some((target.target_step_id.clone(), reason))
}

fn select_transition_rule(
    transitions: &[PlanStepControlFlow],
    run_failed_or_verify_failed: bool,
) -> Option<&PlanStepControlFlow> {
    if run_failed_or_verify_failed {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_fail" || t.condition == "always")
    } else {
        transitions
            .iter()
            .find(|t| t.condition == "on_verify_success" || t.condition == "always")
    }
}

/// 若 `target_step_id` 落在 **`original_steps`** 中：截断当前队列至 `i+1`，追加从目标起的后缀（id 加 `-loop{i}`），返回用户可见反馈正文与 SSE 状态。
pub(crate) fn try_apply_staged_plan_control_flow_jump(
    step: &PlanStepV1,
    i: usize,
    plan_steps: &mut Vec<PlanStepV1>,
    original_steps: &[PlanStepV1],
    transition_counters: &mut HashMap<String, u32>,
    run_failed_or_verify_failed: bool,
    step_verify_failed_reason: &Option<String>,
) -> Option<(String, &'static str)> {
    let (target_id, reason) = staged_step_transition_trigger(
        step,
        run_failed_or_verify_failed,
        step_verify_failed_reason,
        transition_counters,
    )?;
    let target_idx = original_steps.iter().position(|s| s.id == target_id)?;
    let mut new_suffix = original_steps[target_idx..].to_vec();
    let loop_suffix = format!("-loop{i}");
    for s in &mut new_suffix {
        s.id = format!("{}{}", s.id, loop_suffix);
    }
    plan_steps.truncate(i.saturating_add(1));
    plan_steps.extend(new_suffix);
    let fb = format!(
        "### 状态机流转：触发控制流跳转\n\
         根据规划设定的 transitions 规则，由于 [{}]，系统已追加回退或跳转到步骤 `{}` 的执行指令。\n\
         请注意调整接下来的工具调用。",
        reason, target_id
    );
    let sse_status = if run_failed_or_verify_failed {
        "failed"
    } else {
        "ok"
    };
    Some((fb, sse_status))
}

use crate::agent::step_executor_policy::executor_kind_user_label;

/// 注入执行器的单步 **user** 正文（与 `run_staged_plan_steps_loop` 内 `format!` 对齐）。
///
/// `immutable_user_goal`：系统持有的本轮用户原文（不变层）；有则置于分步说明之前以减少步内漂移。
pub(crate) fn staged_injected_step_user_body(
    step_index: usize,
    n: usize,
    step: &PlanStepV1,
    immutable_user_goal: Option<&str>,
) -> String {
    let immutable_prefix = immutable_user_goal
        .filter(|g| !g.trim().is_empty())
        .map(crate::agent::plan_optimizer::staged_rolling_immutable_step_user_prefix)
        .unwrap_or_default();
    let summary_hint = if step_index == n && n > 1 {
        format!(
            "\n本步为最后一步，终答中请简要列出本轮全部 {} 个步骤的完成情况（可对每步附简短说明）。",
            n
        )
    } else {
        String::new()
    };
    let sub_agent_hint = match step.executor_kind {
        Some(k) => format!(
            "\n- **子代理角色**（本步 `tools` 已按策略表收窄）：`{}` — {}\n",
            k.as_snake_case_str(),
            executor_kind_user_label(k)
        ),
        None => String::new(),
    };
    format!(
        "{immutable_prefix}### 分步 {}/{}\n{}{}{}\n- id: {}\n- 描述: {}",
        step_index,
        n,
        crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
        summary_hint,
        sub_agent_hint,
        step.id,
        step.description
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::{PlanStepExecutorKind, PlanStepV1};

    fn step_with_transition(
        id: &str,
        condition: &str,
        target: &str,
        max_loops: Option<u32>,
    ) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: "d".to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: Some(vec![PlanStepControlFlow {
                condition: condition.to_string(),
                target_step_id: target.to_string(),
                max_loops,
            }]),
        }
    }

    #[test]
    fn transition_respects_max_loops() {
        let step = step_with_transition("a", "on_verify_success", "b", Some(1));
        let mut counters = HashMap::new();
        assert!(staged_step_transition_trigger(&step, false, &None, &mut counters).is_some());
        assert!(staged_step_transition_trigger(&step, false, &None, &mut counters).is_none());
    }

    #[test]
    fn jump_truncates_and_suffixes_ids() {
        let original = vec![
            PlanStepV1 {
                id: "s0".into(),
                description: "".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            },
            PlanStepV1 {
                id: "s1".into(),
                description: "".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            },
        ];
        let mut plan_steps = original.clone();
        let step = step_with_transition("cur", "always", "s1", None);
        let mut counters = HashMap::new();
        let r = try_apply_staged_plan_control_flow_jump(
            &step,
            0,
            &mut plan_steps,
            original.as_slice(),
            &mut counters,
            false,
            &None,
        );
        assert!(r.is_some());
        assert_eq!(plan_steps.len(), 2);
        assert_eq!(plan_steps[0].id, "s0");
        assert_eq!(plan_steps[1].id, "s1-loop0");
    }

    #[test]
    fn unknown_target_returns_none_without_truncating() {
        let original = vec![PlanStepV1 {
            id: "only".into(),
            description: "".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }];
        let mut plan_steps = vec![
            PlanStepV1 {
                id: "a".into(),
                description: "".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            },
            PlanStepV1 {
                id: "b".into(),
                description: "".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            },
        ];
        let step = step_with_transition("x", "always", "missing", None);
        let mut counters = HashMap::new();
        let before_len = plan_steps.len();
        let r = try_apply_staged_plan_control_flow_jump(
            &step,
            0,
            &mut plan_steps,
            original.as_slice(),
            &mut counters,
            false,
            &None,
        );
        assert!(r.is_none());
        assert_eq!(plan_steps.len(), before_len);
    }

    #[test]
    fn injected_body_contains_step_meta() {
        let step = PlanStepV1 {
            id: "sid".into(),
            description: "desc".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        let body = staged_injected_step_user_body(1, 3, &step, None);
        assert!(body.contains("### 分步 1/3"));
        assert!(body.contains("sid"));
        assert!(body.contains("desc"));
        assert!(body.contains("review_readonly"));
    }

    #[test]
    fn injected_body_prefixes_immutable_goal_when_present() {
        let step = PlanStepV1 {
            id: "sid".into(),
            description: "desc".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        };
        let body = staged_injected_step_user_body(1, 2, &step, Some("用户总问句"));
        assert!(body.contains("【不变层·本轮用户总目标】"));
        assert!(body.contains("用户总问句"));
        assert!(body.contains("### 分步 1/2"));
    }
}
