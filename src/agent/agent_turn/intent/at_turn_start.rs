//! 在 `run_agent_turn` 起点（**非** Hierarchical 模式可选）与分层模式共用的**意图门控**：
//! 默认 L2；旧 L0/L1 规则层仅在 L2 不可用时兜底。非「直接执行」时写入助手终答并结束本回合。
//! `meta.greeting`、`qa.meta*`、`qa.explain`、`qa.readonly*`（只读 + hint）、`ClarifyThenExecute`、`ConfirmThenExecute` 等改入**主模型**；占位 canned 不终答（可配合 `intent_turn_gate_hint` 与 `system_intent_gate_hint`）。

use crate::agent::intent_pipeline::IntentAction;
use crate::agent::intent_router::{
    ExecuteIntentThresholds, intent_reply_delegates_to_main_model, qa_readonly_style_primary,
};
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::sse;
use crabmate_agent::agent_turn::{
    IntentGateSnapshot, IntentRoutingPipelineParams, assess_intent_routing_full_pipeline,
    intent_gate_snapshot_finished_early, intent_gate_snapshot_from_decision,
};

use super::super::params::RunLoopParams;
use super::intent_user;
use super::l2_classifier_host::CrabmateIntentL2ClassifierHost;

/// 只读门控：主模型作答 + 工具已收窄为只读。
const GATE_HINT_READONLY_ZH: &str = "【意图门控】当前回合应只读理解仓库（可列出/读取文件），不要改文件、不要跑测试或长耗时构建，除非用户明确要求。勿将用户宽泛诉求静默收窄为单点深层修复；若拟切换任务粒度须先征得用户同意。";
/// 模糊意图：主模型追问而非 canned 模板。
const GATE_HINT_CLARIFY_ZH: &str = "【意图门控】用户目标可能不够明确。请用简短自然的中文追问：尽量请用户补充文件路径、报错原文、拟运行命令或期望结果；不要编造未提供的信息；追问勿预设对方只想修某一个文件。此轮不要执行会修改仓库或长耗时构建的操作，除非用户已明确授权。";
/// 待确认执行：主模型说明 + 保留可识别的确认措辞（见 `intent_router::is_waiting_execute_confirmation_prompt`）。
const GATE_HINT_CONFIRM_ZH: &str = "【意图门控】你判断用户可能想让你执行具体改动或命令，但需要先确认。请简短说明你的理解（与用户原文目标粒度保持一致，勿擅自替换为更小范围任务除非用户已表态）；并在回复最后一段包含可被识别的确认句式，例如同时包含「请确认是否」与「开始执行」或「直接开始执行」（可与历史文案「请确认是否「直接开始执行」」同义），以便多轮对话继续识别确认流。";

/// 意图门控的聚合结果：终答结束本回合，或进入主执行路径（**Execute**、`qa.readonly` 续接、以及委托主模型的 **`DirectReply`** 等会进入主路径）。
pub(crate) enum IntentGateResult {
    /// 已写入助手终答，调用方应结束本回合。
    Finished,
    /// L2 判定为直接执行，可继续主循环/分层 Runner（兜底规则层细节已写入 `intent_analysis` 时间线）。
    ProceedExecute {
        #[allow(dead_code)]
        assessment: crate::agent::intent_pipeline::IntentDecision,
    },
}

/// `false` 表示本回合已写入助手终答，调用方应 `return Ok(())`。
pub(crate) async fn run_intent_at_turn_start_if_configured(
    p: &mut RunLoopParams<'_>,
) -> Result<bool, super::super::errors::RunAgentTurnError> {
    let in_clarification_flow =
        intent_user::recently_waiting_execute_confirmation(p.turn.messages());
    let task = intent_user::extract_effective_user_task(p.turn.messages(), in_clarification_flow);
    if task.trim().is_empty() {
        p.turn.turn_planner_hints.intent_gate_snapshot = Some(IntentGateSnapshot::EmptyTask);
        return Ok(true);
    }
    let out = run_intent_l0_l1_l2_gate(
        p,
        &task,
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_low_threshold,
            high: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_high_threshold,
        },
        "intent_at_turn",
    )
    .await?;
    // 始终运行意图管线 & 发射时间线。intent_at_turn_start_enabled 仅控制门控是否提前终答。
    if !p.ctx.core.cfg.intent_routing.intent_at_turn_start_enabled {
        p.turn.turn_planner_hints.intent_gate_snapshot = Some(IntentGateSnapshot::Disabled);
        return Ok(true);
    }
    let proceed = matches!(out, IntentGateResult::ProceedExecute { .. });
    if proceed {
        p.turn
            .turn_planner_hints
            .suppress_duplicate_intent_timeline_once = true;
    }
    Ok(proceed)
}

fn format_intent_title(assessment: &crate::agent::intent_pipeline::IntentDecision) -> String {
    let kind = match assessment.kind {
        crate::agent::intent_router::IntentKind::Greeting => "问候类",
        crate::agent::intent_router::IntentKind::Execute => "执行类",
        crate::agent::intent_router::IntentKind::Qa => "问答类",
        crate::agent::intent_router::IntentKind::Ambiguous => "待澄清",
    };
    let action = match &assessment.action {
        IntentAction::Execute => "直接执行",
        IntentAction::ConfirmThenExecute(_) => "确认后执行",
        IntentAction::ClarifyThenExecute(_) => "澄清后执行",
        IntentAction::DirectReply(_) => "直接回复",
    };
    format!("意图分析：{}（{}）", kind, action)
}

fn format_intent_detail(
    assessment: &crate::agent::intent_pipeline::IntentDecision,
    merge_meta: &crate::agent::intent_pipeline::IntentMergeMeta,
) -> String {
    let l0 = &merge_meta.l0;
    let l2_status = if !merge_meta.l2_present {
        match merge_meta.l2_unavailable_reason.as_deref() {
            Some(reason) => format!("L2 不可用（原因：{reason}），使用弃用规则层兜底"),
            None => "L2 不可用，使用弃用规则层兜底".to_string(),
        }
    } else if merge_meta.l2_applied {
        match merge_meta.l2_confidence {
            Some(c) => format!("L2（置信度 {:.2}）", c),
            None => "L2".to_string(),
        }
    } else {
        match merge_meta.l2_confidence {
            Some(c) => format!("弃用规则层兜底（L2 未采纳，置信度 {:.2}）", c),
            None => "弃用规则层兜底".to_string(),
        }
    };
    let override_reason = merge_meta
        .override_reason
        .as_deref()
        .unwrap_or("无")
        .to_string();
    let secondary = if assessment.secondary_intents.is_empty() {
        "无".to_string()
    } else {
        assessment.secondary_intents.join("、")
    };
    format!(
        "主意图：{}\n次意图：{}\n综合置信度：{:.2}\n需要澄清：{}\n是否保守拒识：{}\n弃用规则判定：{:?}（{:.2}）\n决策来源：{}\n来源原因：{}\n是否命中续接合并：{}\nL0 信号：路径={}，报错={}，短句={}，Git关键词={}，命令词={}，近期工具失败={}",
        assessment.primary_intent,
        secondary,
        assessment.confidence,
        assessment.need_clarification,
        assessment.abstain,
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        l2_status,
        override_reason,
        merge_meta.used_merged_continuation,
        l0.has_file_path_like,
        l0.has_error_signal,
        l0.is_short,
        l0.has_git_keyword,
        l0.has_command_cargo,
        l0.has_recent_tool_failure
    )
}

/// 推送 `intent_analysis` 时间线（供开局门控与 **非分层** `staged_plan_intent_gate` 共用）。
pub(crate) async fn emit_intent_timeline_gate_only(
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    sse_log_context: &'static str,
    assessment: &crate::agent::intent_pipeline::IntentDecision,
    merge_meta: &crate::agent::intent_pipeline::IntentMergeMeta,
) {
    let Some(out) = out else {
        return;
    };
    let _ = sse::send_string_logged(
        out,
        sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "intent_analysis".to_string(),
                title: format_intent_title(assessment),
                detail: Some(format_intent_detail(assessment, merge_meta)),
            },
        }),
        sse_log_context,
    )
    .await;
}

/// 推送与分层一致的终答并结束本回合。返回 `Ok(false)` 表示本回合不再进主执行。
async fn apply_non_execute_and_finish(
    p: &mut RunLoopParams<'_>,
    reply: &str,
) -> Result<bool, super::super::errors::RunAgentTurnError> {
    p.turn
        .push_message(crate::types::Message::assistant_only(reply.to_string()));
    if let Some(out) = p.ctx.io.out {
        // 关闭 reasoning 生命周期，开启 text 生命周期
        crate::sse::send_reasoning_message_end_sse(out, "reasoning").await;
        let message_id = "msg-assistant-intent";
        crate::sse::send_text_message_start_sse(out, message_id, "assistant").await;
        let phase = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let _ = sse::send_string_logged(out, phase, "intent::answer_phase").await;
        let _ =
            sse::send_string_logged(out, reply.to_string(), "intent::final_response_delta").await;
        let final_tl = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "final_response".to_string(),
                title: reply.to_string(),
                detail: None,
            },
        });
        let _ = sse::send_string_logged(out, final_tl, "intent::final_response").await;
        crate::sse::send_text_message_end_sse(out, message_id).await;
    }
    Ok(false)
}

async fn run_intent_l0_l1_l2_gate(
    p: &mut RunLoopParams<'_>,
    task: &str,
    in_clarification_flow: bool,
    thresholds: ExecuteIntentThresholds,
    sse_log_tag: &'static str,
) -> Result<IntentGateResult, super::super::errors::RunAgentTurnError> {
    let host = CrabmateIntentL2ClassifierHost {
        cfg: p.ctx.core.cfg.as_ref(),
        llm_backend: p.ctx.core.llm_backend,
        client: p.ctx.core.client,
        api_key: p.ctx.core.api_key,
        turn_budget: Some(&p.turn.turn_budget),
    };
    let outcome = assess_intent_routing_full_pipeline(
        &host,
        &IntentRoutingPipelineParams {
            task,
            messages: p.turn.messages(),
            cfg: p.ctx.core.cfg.as_ref(),
            in_clarification_flow,
            thresholds,
            l2_enabled: p.ctx.core.cfg.intent_routing.intent_l2_enabled,
            sse_log_tag,
        },
    )
    .await;
    p.turn.turn_planner_hints.intent_routing_cache =
        Some(super::super::params::IntentRoutingCacheEntry {
            task: task.to_string(),
            decision: outcome.decision.clone(),
            merge_meta: outcome.merge_meta.clone(),
        });
    let assessment = outcome.decision;
    emit_intent_timeline_gate_only(p.ctx.io.out, sse_log_tag, &assessment, &outcome.merge_meta)
        .await;

    if let Some(constraints) = infer_turn_execution_constraints(task)
        && constraints.requires_review_readonly()
    {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(PlanStepExecutorKind::ReviewReadonly);
        p.turn.turn_planner_hints.intent_turn_gate_hint = Some(constraints.intent_gate_hint_zh());
        p.turn.turn_planner_hints.intent_gate_snapshot =
            Some(intent_gate_snapshot_from_decision(&assessment));
        return Ok(IntentGateResult::ProceedExecute { assessment });
    }

    if matches!(assessment.action, IntentAction::Execute) {
        p.turn.turn_planner_hints.intent_gate_snapshot =
            Some(intent_gate_snapshot_from_decision(&assessment));
        return Ok(IntentGateResult::ProceedExecute { assessment });
    }

    if assessment.kind == crate::agent::intent_router::IntentKind::Qa
        && qa_readonly_style_primary(&assessment.primary_intent)
        && matches!(&assessment.action, IntentAction::DirectReply(_))
    {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(PlanStepExecutorKind::ReviewReadonly);
        p.turn.turn_planner_hints.intent_turn_gate_hint = Some(GATE_HINT_READONLY_ZH.to_string());
        p.turn.turn_planner_hints.intent_gate_snapshot =
            Some(intent_gate_snapshot_from_decision(&assessment));
        return Ok(IntentGateResult::ProceedExecute { assessment });
    }

    if matches!(&assessment.action, IntentAction::DirectReply(_))
        && intent_reply_delegates_to_main_model(assessment.kind, &assessment.primary_intent)
    {
        p.turn.turn_planner_hints.intent_gate_snapshot =
            Some(intent_gate_snapshot_from_decision(&assessment));
        return Ok(IntentGateResult::ProceedExecute { assessment });
    }

    match &assessment.action {
        IntentAction::ClarifyThenExecute(_) => {
            p.turn.turn_planner_hints.intent_turn_gate_hint =
                Some(GATE_HINT_CLARIFY_ZH.to_string());
            p.turn.turn_planner_hints.step_executor_constraint =
                Some(PlanStepExecutorKind::ReviewReadonly);
            p.turn.turn_planner_hints.intent_gate_snapshot =
                Some(intent_gate_snapshot_from_decision(&assessment));
            return Ok(IntentGateResult::ProceedExecute { assessment });
        }
        IntentAction::ConfirmThenExecute(_) => {
            p.turn.turn_planner_hints.intent_turn_gate_hint =
                Some(GATE_HINT_CONFIRM_ZH.to_string());
            p.turn.turn_planner_hints.step_executor_constraint =
                Some(PlanStepExecutorKind::ReviewReadonly);
            p.turn.turn_planner_hints.intent_gate_snapshot =
                Some(intent_gate_snapshot_from_decision(&assessment));
            return Ok(IntentGateResult::ProceedExecute { assessment });
        }
        _ => {}
    }

    match assessment.action {
        IntentAction::DirectReply(ref s) => {
            p.turn.turn_planner_hints.intent_gate_snapshot =
                Some(intent_gate_snapshot_finished_early(&assessment));
            let _ = apply_non_execute_and_finish(p, s).await?;
            Ok(IntentGateResult::Finished)
        }
        IntentAction::ClarifyThenExecute(_) | IntentAction::ConfirmThenExecute(_) => {
            unreachable!("clarify/confirm branch returns above")
        }
        IntentAction::Execute => unreachable!("execute branch returns above"),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TurnExecutionConstraints {
    no_write: bool,
    no_command_execution: bool,
    analysis_only: bool,
    ask_before_mutation: bool,
}

impl TurnExecutionConstraints {
    fn requires_review_readonly(self) -> bool {
        self.analysis_only && (self.no_write || self.no_command_execution)
    }

    fn intent_gate_hint_zh(self) -> String {
        let mut limits = Vec::new();
        if self.no_write {
            limits.push("不得修改文件、不得继续 patch");
        }
        if self.no_command_execution {
            limits.push("不得运行构建/测试/执行类命令");
        }
        if self.analysis_only {
            limits.push("以只读诊断、原因分析和操作说明为主");
        }
        if self.ask_before_mutation {
            limits.push("如需再次执行或修改，必须先说明原因并取得用户确认");
        }
        format!(
            "【意图门控】用户本轮给出了执行约束：{}。当前回合按只读诊断处理：可以读取/列目录/解释失败原因，但不要越过上述约束。",
            limits.join("；")
        )
    }
}

fn infer_turn_execution_constraints(task: &str) -> Option<TurnExecutionConstraints> {
    let t = task.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let no_write = [
        "取消修复",
        "不用修复",
        "不要修复",
        "别修复",
        "不要修改",
        "别修改",
        "先别改",
        "先不要改",
        "不改代码",
        "without modifying",
        "do not modify",
        "don't modify",
        "no patch",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let no_command_execution = [
        "不要运行",
        "别运行",
        "不要执行",
        "别执行",
        "不要编译",
        "别编译",
        "不要跑",
        "别跑",
        "do not run",
        "don't run",
        "without running",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let analysis_only = [
        "分析",
        "诊断",
        "说明",
        "怎么编译",
        "如何编译",
        "怎么做",
        "只读",
        "只分析",
        "analyze",
        "diagnose",
        "explain",
        "how to",
        "readonly",
        "read-only",
    ]
    .iter()
    .any(|marker| t.contains(marker));
    let ask_before_mutation = no_write || t.contains("先问我") || t.contains("先确认");
    let constraints = TurnExecutionConstraints {
        no_write,
        no_command_execution: no_command_execution || (no_write && analysis_only),
        analysis_only,
        ask_before_mutation,
    };
    (constraints != TurnExecutionConstraints::default()).then_some(constraints)
}

#[cfg(test)]
mod tests {
    use super::infer_turn_execution_constraints;

    #[test]
    fn infers_readonly_constraints_from_cancel_fix_analyze_request() {
        let c = infer_turn_execution_constraints(
            "应该不用修改就可以编译，先取消修复，然后分析一下怎么编译",
        )
        .expect("constraints");
        assert!(c.no_write);
        assert!(c.no_command_execution);
        assert!(c.analysis_only);
        assert!(c.requires_review_readonly());

        let c =
            infer_turn_execution_constraints("不要修改文件，只分析失败原因").expect("constraints");
        assert!(c.requires_review_readonly());
    }

    #[test]
    fn infers_command_execution_constraint() {
        let c = infer_turn_execution_constraints("先不要运行测试，只分析一下失败原因")
            .expect("constraints");
        assert!(c.no_command_execution);
        assert!(c.analysis_only);
        assert!(c.requires_review_readonly());
    }

    #[test]
    fn does_not_mark_plain_build_request_readonly() {
        assert!(infer_turn_execution_constraints("编译 hpcg").is_none());
        let c = infer_turn_execution_constraints("分析当前项目").expect("analysis constraint");
        assert!(c.analysis_only);
        assert!(!c.requires_review_readonly());
    }
}
