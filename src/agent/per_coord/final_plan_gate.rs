//! 终答 `agent_reply_plan` v1 门控：将「是否需要规划 / 静态校验 / 重写或耗尽」收拢为显式分支（见 `docs/design/per_state_machine_consolidation.md`）。
//! **不**修改 `messages`；侧向语义 LLM 仍由调用方在收到 `StopTurnPendingPlanConsistencyLlm` 后执行。

use crate::agent::plan_artifact;
use crate::agent::reflection::plan_rewrite;
use crate::config::AgentConfig;
use crate::types::Message;

use super::{AfterFinalAssistant, FinalPlanRequirementMode, PlanRequirementSource};

/// 工作流反思控制器的轮次（仅用于日志），与门控逻辑无耦合。
pub(crate) struct FinalPlanGateArgs<'a> {
    pub msg: &'a Message,
    pub messages: &'a [Message],
    pub cfg: &'a AgentConfig,
    pub workspace_is_set: bool,
    pub final_plan_policy: FinalPlanRequirementMode,
    pub plan_requirement_source: PlanRequirementSource,
    pub final_plan_require_strict_workflow_node_coverage: bool,
    pub final_plan_semantic_check_enabled: bool,
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 已由 `PerCoordinator::workflow_validate_layer_need` 解析；与 `messages` 长度缓存一致。
    pub layer_need: Option<usize>,
    pub validate_only_binding_ids: Option<Vec<String>>,
    pub plan_rewrite_attempts: usize,
    pub plan_rewrite_max_attempts: usize,
}

/// 在「需要终答规划」前提下，按 **final_assistant** 事件推进门控，返回 `AfterFinalAssistant`。
/// 调用方须在返回 `RequestPlanRewrite` 后自行将 `plan_rewrite_attempts` 与之一致（本函数**不**修改计数，与旧实现一致：先判断耗尽再 `+=1`）。
pub(crate) fn step_after_require_plan(
    args: FinalPlanGateArgs<'_>,
) -> (AfterFinalAssistant, Option<usize>) {
    let apply_layer_semantics = match args.final_plan_policy {
        FinalPlanRequirementMode::Never => false,
        FinalPlanRequirementMode::WorkflowReflection => {
            args.plan_requirement_source == PlanRequirementSource::WorkflowReflection
        }
        FinalPlanRequirementMode::Always => true,
    };
    let layer_need = args.layer_need;
    let validate_only_binding_ids = &args.validate_only_binding_ids;

    let content = crate::types::message_content_as_str(&args.msg.content).unwrap_or("");
    if let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1_with_validate_only_binding_ids(
        content,
        validate_only_binding_ids.as_deref(),
    ) {
        let layers_ok = match layer_need {
            Some(n) if n > 0 && apply_layer_semantics => plan.steps.len() >= n,
            _ => true,
        };
        let wf_ids = plan_rewrite::last_workflow_tool_node_ids(args.messages);
        let workflow_subset_ok = match wf_ids.as_ref() {
            Some(ids) => plan_artifact::validate_plan_workflow_node_ids_subset(&plan, ids).is_ok(),
            None => true,
        };
        let workflow_cover_ok = if args.final_plan_require_strict_workflow_node_coverage {
            match wf_ids.as_ref() {
                Some(ids) => {
                    plan_artifact::validate_plan_covers_all_workflow_node_ids(&plan, ids).is_ok()
                }
                None => true,
            }
        } else {
            true
        };
        let workflow_ids_ok = workflow_subset_ok && workflow_cover_ok;
        let validate_only_binding_ok = if apply_layer_semantics {
            match validate_only_binding_ids.as_ref() {
                Some(ids) if !ids.is_empty() => {
                    plan_artifact::validate_plan_binds_workflow_validate_nodes(&plan, ids).is_ok()
                }
                _ => true,
            }
        } else {
            true
        };
        if layers_ok && workflow_ids_ok && validate_only_binding_ok {
            let digest = plan_rewrite::summarize_messages_for_final_plan_semantic_check(
                args.messages,
                args.cfg,
                args.workspace_is_set,
                args.final_plan_semantic_check_max_non_readonly_tools,
            );
            let want_llm = args.final_plan_semantic_check_enabled
                && matches!(
                    args.final_plan_policy,
                    FinalPlanRequirementMode::WorkflowReflection
                )
                && args.plan_requirement_source == PlanRequirementSource::WorkflowReflection
                && digest.is_some();
            if want_llm {
                log::info!(
                    target: "crabmate::per",
                    "after_final_assistant outcome=pending_plan_consistency_llm plan_steps={} layer_need={:?}",
                    plan.steps.len(),
                    layer_need
                );
                return (
                    AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm {
                        plan,
                        tool_digest: digest,
                    },
                    None,
                );
            }
            log::info!(
                target: "crabmate::per",
                "after_final_assistant outcome=stop_plan_ok plan_steps={} layer_need={:?}",
                plan.steps.len(),
                layer_need
            );
            return (AfterFinalAssistant::StopTurn, None);
        }
        log::info!(
            target: "crabmate::per",
            "after_final_assistant outcome=plan_schema_ok_semantics_fail plan_steps={} layer_need={:?} workflow_node_ids_ok={} validate_only_binding_ok={}",
            plan.steps.len(),
            layer_need,
            workflow_ids_ok,
            validate_only_binding_ok
        );
    }

    if args.plan_rewrite_attempts >= args.plan_rewrite_max_attempts {
        let reason = plan_rewrite::classify_exhausted_reason(
            args.msg,
            args.messages,
            layer_need,
            apply_layer_semantics,
            args.final_plan_require_strict_workflow_node_coverage,
        );
        log::warn!(
            target: "crabmate::per",
            "after_final_assistant outcome=plan_rewrite_exhausted layer_need={:?} reason={:?}",
            layer_need,
            reason
        );
        return (
            AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason },
            None,
        );
    }
    let next_attempt = args.plan_rewrite_attempts + 1;
    let validate_only_bind_ids = validate_only_binding_ids.as_ref().filter(|v| !v.is_empty());
    let bind_suffix = validate_only_bind_ids
        .map(|ids| plan_rewrite::validate_only_plan_binding_rewrite_suffix(ids.as_slice()))
        .unwrap_or_default();
    let rewrite_text = match (
        layer_need.filter(|&n| n > 0 && apply_layer_semantics),
        plan_rewrite::last_workflow_tool_node_ids(args.messages),
    ) {
        (Some(n), Some(ids)) if !ids.is_empty() => {
            let strict = if args.final_plan_require_strict_workflow_node_coverage {
                format!(
                    "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                    ids.join(", ")
                )
            } else {
                String::new()
            };
            format!(
                "{}\n\n补充：\n- 最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。\n- 若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与 `nodes[].id` 对齐）：{}。{}",
                plan_rewrite::plan_rewrite_user_text_base(),
                ids.join(", "),
                strict
            )
        }
        (Some(n), _) => format!(
            "{}\n\n补充：最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。",
            plan_rewrite::plan_rewrite_user_text_base()
        ),
        (None, Some(ids)) if !ids.is_empty() => {
            let strict = if args.final_plan_require_strict_workflow_node_coverage {
                format!(
                    "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                    ids.join(", ")
                )
            } else {
                String::new()
            };
            format!(
                "{}\n\n补充：若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与最近一次 `workflow_execute` 工具结果中 `nodes[].id` 对齐）：{}。{}",
                plan_rewrite::plan_rewrite_user_text_base(),
                ids.join(", "),
                strict
            )
        }
        (None, _) => plan_rewrite::plan_rewrite_user_text_base(),
    };
    let rewrite_text = format!("{rewrite_text}{bind_suffix}");
    log::info!(
        target: "crabmate::per",
        "after_final_assistant outcome=request_plan_rewrite attempt={} layer_need={:?}",
        next_attempt,
        layer_need
    );
    (
        AfterFinalAssistant::RequestPlanRewrite(Message {
            role: "user".to_string(),
            content: Some(rewrite_text.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }),
        Some(next_attempt),
    )
}

/// 对一次终答 assistant 运行完整门控（含 `require_plan == false` 的早停）。
pub(crate) fn after_final_assistant(
    per: &mut super::PerCoordinator,
    msg: &Message,
    messages: &[Message],
    cfg: &AgentConfig,
    workspace_is_set: bool,
) -> AfterFinalAssistant {
    let require_plan = match per.final_plan_policy {
        FinalPlanRequirementMode::Never => false,
        FinalPlanRequirementMode::WorkflowReflection => {
            per.plan_requirement_source == PlanRequirementSource::WorkflowReflection
        }
        FinalPlanRequirementMode::Always => true,
    };

    log::info!(
        target: "crabmate::per",
        "after_final_assistant enter policy={:?} require_plan={} plan_requirement_source={:?} reflection_stage_round={} plan_rewrite_attempts={} plan_rewrite_max={}",
        per.final_plan_policy,
        require_plan,
        per.plan_requirement_source,
        per.reflection.stage_round(),
        per.plan_rewrite_attempts,
        per.plan_rewrite_max_attempts
    );

    if !require_plan {
        log::info!(
            target: "crabmate::per",
            "after_final_assistant outcome=stop_no_requirement"
        );
        return AfterFinalAssistant::StopTurn;
    }

    let layer_need = per.workflow_validate_layer_need(messages);
    let validate_only_binding_ids =
        plan_rewrite::last_workflow_validate_binding_plan_node_ids(messages);

    let (out, next_count) = step_after_require_plan(FinalPlanGateArgs {
        msg,
        messages,
        cfg,
        workspace_is_set,
        final_plan_policy: per.final_plan_policy,
        plan_requirement_source: per.plan_requirement_source,
        final_plan_require_strict_workflow_node_coverage: per
            .final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled: per.final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools: per
            .final_plan_semantic_check_max_non_readonly_tools,
        layer_need,
        validate_only_binding_ids,
        plan_rewrite_attempts: per.plan_rewrite_attempts,
        plan_rewrite_max_attempts: per.plan_rewrite_max_attempts,
    });
    if let Some(n) = next_count {
        per.plan_rewrite_attempts = n;
    }
    out
}
