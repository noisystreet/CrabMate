use std::collections::{HashMap, HashSet};

use super::{AgentReplyPlanV1, PlanArtifactError, plan_step_id_syntax_ok};

/// 校验规划中出现的 `workflow_node_id` 均为 `workflow_node_ids` 的子集（通常来自最近一次 `workflow_execute` 工具结果的 `nodes[].id`）。
pub(crate) fn validate_plan_workflow_node_ids_subset(
    plan: &AgentReplyPlanV1,
    workflow_node_ids: &[String],
) -> Result<(), PlanArtifactError> {
    let set: HashSet<&str> = workflow_node_ids.iter().map(|s| s.as_str()).collect();
    for (i, s) in plan.steps.iter().enumerate() {
        let Some(ref w) = s.workflow_node_id else {
            continue;
        };
        let w = w.trim();
        if !set.contains(w) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "workflow_node_id 不在最近一次工作流工具结果的 nodes 列表中",
            });
        }
    }
    Ok(())
}

/// 若规划中**至少一步**含 `workflow_node_id`，则 `steps` 中出现的 `workflow_node_id` 须**覆盖** `workflow_node_ids` 全部节点（每 id 至少一步引用）。
pub(crate) fn validate_plan_covers_all_workflow_node_ids(
    plan: &AgentReplyPlanV1,
    workflow_node_ids: &[String],
) -> Result<(), PlanArtifactError> {
    if workflow_node_ids.is_empty() {
        return Ok(());
    }
    let any_linked = plan.steps.iter().any(|s| s.workflow_node_id.is_some());
    if !any_linked {
        return Ok(());
    }
    let mut covered: HashSet<&str> = HashSet::new();
    for s in &plan.steps {
        if let Some(ref w) = s.workflow_node_id {
            covered.insert(w.trim());
        }
    }
    let mut missing = Vec::new();
    for id in workflow_node_ids {
        let t = id.as_str();
        if !covered.contains(t) {
            missing.push(t.to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(PlanArtifactError::WorkflowNodesNotFullyCovered { missing })
    }
}

/// 在 **`workflow_validate_only`** 路径上强制规划与 **`nodes[].id` 绑定**：`steps.len() == nodes.len()`、每步均有 `workflow_node_id`、二者多重集合一致（顺序可不同）。
///
/// `validate_only_node_ids` 通常来自历史中最近一次 `report_type == workflow_validate_result` 的 `nodes[].id`；为空切片时不校验（无节点则不做绑定）。
pub(crate) fn validate_plan_binds_workflow_validate_nodes(
    plan: &AgentReplyPlanV1,
    validate_only_node_ids: &[String],
) -> Result<(), PlanArtifactError> {
    if validate_only_node_ids.is_empty() {
        return Ok(());
    }
    if plan.steps.len() != validate_only_node_ids.len() {
        return Err(PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch {
            detail: "steps_len_must_equal_nodes_len",
        });
    }
    let mut expected: HashMap<&str, usize> = HashMap::new();
    for id in validate_only_node_ids {
        *expected.entry(id.as_str()).or_insert(0) += 1;
    }
    let mut actual: HashMap<&str, usize> = HashMap::new();
    for s in &plan.steps {
        let Some(ref w) = s.workflow_node_id else {
            return Err(PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch {
                detail: "each_step_requires_workflow_node_id",
            });
        };
        let t = w.trim();
        if t.is_empty() {
            return Err(PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch {
                detail: "each_step_requires_workflow_node_id",
            });
        }
        *actual.entry(t).or_insert(0) += 1;
    }
    if actual == expected {
        Ok(())
    } else {
        Err(PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch {
            detail: "workflow_node_id_multiset_must_match_nodes",
        })
    }
}

pub(super) fn validate_agent_reply_plan_v1(p: &AgentReplyPlanV1) -> Result<(), PlanArtifactError> {
    validate_agent_reply_plan_v1_with_validate_only_binding_ids(p, None)
}

pub(super) fn validate_agent_reply_plan_v1_with_validate_only_binding_ids(
    p: &AgentReplyPlanV1,
    validate_only_binding_ids: Option<&[String]>,
) -> Result<(), PlanArtifactError> {
    const STAGED_PLAN_FIXED_MAX_STEPS: usize = 1;
    if p.plan_type != "agent_reply_plan" {
        return Err(PlanArtifactError::WrongType(p.plan_type.clone()));
    }
    if p.version != 1 {
        return Err(PlanArtifactError::WrongVersion(p.version));
    }
    if p.no_task {
        if !p.steps.is_empty() {
            return Err(PlanArtifactError::NoTaskWithNonEmptySteps);
        }
        return Ok(());
    }
    if p.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    // 固定单步：默认只允许 1 步。
    // 绑定优先例外：若每步都显式绑定 `workflow_node_id`，且存在 validate-only 绑定上下文，
    // 允许多步交由 `validate_plan_binds_workflow_validate_nodes` 做严格的一一对应校验。
    let workflow_bound_multi_step = p.steps.len() > STAGED_PLAN_FIXED_MAX_STEPS
        && p.steps.iter().all(|s| {
            s.workflow_node_id
                .as_deref()
                .map(|w| !w.trim().is_empty())
                .unwrap_or(false)
        });
    let has_validate_only_binding_context =
        validate_only_binding_ids.is_some_and(|ids| !ids.is_empty());
    let allow_workflow_bound_multi_step =
        workflow_bound_multi_step && has_validate_only_binding_context;
    if p.steps.len() > STAGED_PLAN_FIXED_MAX_STEPS && !allow_workflow_bound_multi_step {
        return Err(PlanArtifactError::TooManySteps {
            max: STAGED_PLAN_FIXED_MAX_STEPS,
            got: p.steps.len(),
        });
    }
    let mut seen_step_ids = HashSet::<String>::new();
    for (i, s) in p.steps.iter().enumerate() {
        let raw_id = s.id.as_str();
        if raw_id != raw_id.trim() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 首尾不得含空白",
            });
        }
        let id = raw_id.trim();
        if id.is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 为空",
            });
        }
        if !plan_step_id_syntax_ok(id) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 语法不合法（须 ASCII 字母数字起头，仅含 - _ . /，总长不超过 128）",
            });
        }
        if !seen_step_ids.insert(id.to_string()) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 重复",
            });
        }
        if s.description.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "description 为空",
            });
        }
        if let Some(ref w) = s.workflow_node_id {
            let raw_w = w.as_str();
            if raw_w != raw_w.trim() {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 首尾不得含空白",
                });
            }
            let w = raw_w.trim();
            if w.is_empty() {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 若出现须为非空字符串（否则请省略该字段）",
                });
            }
            if !plan_step_id_syntax_ok(w) {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 语法不合法",
                });
            }
        }

        if let Some(ref transitions) = s.transitions {
            for t in transitions {
                if !seen_step_ids.contains(t.target_step_id.as_str())
                    && !p.steps.iter().any(|st| st.id == t.target_step_id)
                {
                    return Err(PlanArtifactError::InvalidStep {
                        index: i,
                        reason: "transitions 包含不存在的 target_step_id",
                    });
                }
                #[allow(clippy::collapsible_if)]
                if let Some(max) = t.max_loops {
                    if max > 20 {
                        return Err(PlanArtifactError::InvalidStep {
                            index: i,
                            reason: "transitions 中的 max_loops 不得超过 20",
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
