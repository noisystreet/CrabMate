//! 在 `run_agent_turn` 起点（**非** Hierarchical 模式可选）与分层模式共用的**意图门控**：
//! L0 + L1 + 可选 L2；非「直接执行」时写入助手终答并结束本回合。

use crate::agent::intent_l0;
use crate::agent::intent_l2_classifier::classify_intent_l2_with_llm;
use crate::agent::intent_pipeline::{
    IntentAction, IntentContext, assess_and_route_with_l2, prepare_intent_routing,
};
use crate::agent::intent_router::ExecuteIntentThresholds;
use crate::sse;

use super::intent_user;
use super::params::RunLoopParams;

const RECENT_USER_FOR_MERGE: usize = 4;
const MSG_TAIL_FOR_TOOL: usize = 32;

/// 意图门控的聚合结果：终答结束本回合，或进入主执行路径（当前仅 **Execute** 会进入主路径）。
pub(crate) enum IntentGateResult {
    /// 已写入助手终答，调用方应结束本回合。
    Finished,
    /// L1/L2 判定为直接执行，可继续主循环/分层 Runner（L0/L1/L2 细节已写入 `intent_analysis` 时间线）。
    ProceedExecute {
        assessment: crate::agent::intent_pipeline::IntentDecision,
    },
}

/// `false` 表示本回合已写入助手终答，调用方应 `return Ok(())`。
pub(crate) async fn run_intent_at_turn_start_if_configured(
    p: &mut RunLoopParams<'_>,
) -> Result<bool, super::errors::RunAgentTurnError> {
    if !p.cfg.intent_at_turn_start_enabled {
        return Ok(true);
    }
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(p.messages);
    let task = intent_user::extract_effective_user_task(p.messages, in_clarification_flow);
    if task.trim().is_empty() {
        return Ok(true);
    }
    let out = run_intent_l0_l1_l2_gate(p, &task, in_clarification_flow, "intent_at_turn").await?;
    Ok(matches!(out, IntentGateResult::ProceedExecute { .. }))
}

/// 分层模式在命中有效 user 任务后**总是**走完整 L0/L1/可选 L2（与 `intent_at_turn_start` 无关），
/// 以便 L0 续接、工具失败信号与合并路由文本与 `single_agent` 一致；`ProceedExecute` 供 `HierarchyRunnerParams` 使用。
pub(crate) async fn run_intent_for_hierarchical(
    p: &mut RunLoopParams<'_>,
    task: &str,
) -> Result<IntentGateResult, super::errors::RunAgentTurnError> {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(p.messages);
    run_intent_l0_l1_l2_gate(p, task, in_clarification_flow, "hierarchical::intent").await
}

fn format_intent_title(assessment: &crate::agent::intent_pipeline::IntentDecision) -> String {
    use crate::agent::intent_pipeline::IntentAction;
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
        "未启用/未触发".to_string()
    } else if merge_meta.l2_applied {
        match merge_meta.l2_confidence {
            Some(c) => format!("已应用（置信度 {:.2}）", c),
            None => "已应用".to_string(),
        }
    } else {
        match merge_meta.l2_confidence {
            Some(c) => format!("未应用（置信度 {:.2}）", c),
            None => "未应用".to_string(),
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
        "主意图：{}\n次意图：{}\n综合置信度：{:.2}\n需要澄清：{}\n是否保守拒识：{}\nL1 判定：{:?}（{:.2}）\nL2 结果：{}\n覆盖原因：{}\n是否命中续接合并：{}\nL0 信号：路径={}，报错={}，短句={}，Git关键词={}，命令词={}，近期工具失败={}",
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

async fn emit_intent_timeline(
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
) -> Result<bool, super::errors::RunAgentTurnError> {
    p.messages
        .push(crate::types::Message::assistant_only(reply.to_string()));
    if let Some(out) = p.out {
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
    }
    Ok(false)
}

async fn run_intent_l0_l1_l2_gate(
    p: &mut RunLoopParams<'_>,
    task: &str,
    in_clarification_flow: bool,
    sse_log_tag: &'static str,
) -> Result<IntentGateResult, super::errors::RunAgentTurnError> {
    let has_recent_tool_failure =
        intent_l0::messages_have_recent_tool_failure(p.messages, MSG_TAIL_FOR_TOOL);
    let recent_user_messages =
        intent_user::collect_recent_user_messages(p.messages, RECENT_USER_FOR_MERGE);
    let intent_ctx = IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds: ExecuteIntentThresholds {
            low: p.cfg.intent_execute_low_threshold,
            high: p.cfg.intent_execute_high_threshold,
        },
        l2_min_confidence: p.cfg.intent_l2_min_confidence,
        has_recent_tool_failure,
        l0_routing_boost_enabled: p.cfg.intent_l0_routing_boost_enabled,
    };
    let (routing_for_l1, _, _) = prepare_intent_routing(task, &intent_ctx);
    let l2_candidate = if p.cfg.intent_l2_enabled {
        classify_intent_l2_with_llm(
            &routing_for_l1,
            task,
            p.cfg.as_ref(),
            p.llm_backend,
            p.client,
            p.api_key,
        )
        .await
    } else {
        None
    };
    let (assessment, merge_meta) = assess_and_route_with_l2(task, &intent_ctx, l2_candidate);
    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] {} l1_kind={:?} l1_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} override={:?} final_kind={:?} primary={} conf={:.2} abstain={} need_clarif={} action={:?} merged_continuation={}",
        sse_log_tag,
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.override_reason,
        assessment.kind,
        assessment.primary_intent,
        assessment.confidence,
        assessment.abstain,
        assessment.need_clarification,
        &assessment.action,
        merge_meta.used_merged_continuation,
    );
    emit_intent_timeline(p.out, sse_log_tag, &assessment, &merge_meta).await;

    match assessment.action {
        IntentAction::Execute => Ok(IntentGateResult::ProceedExecute { assessment }),
        IntentAction::DirectReply(ref s)
        | IntentAction::ClarifyThenExecute(ref s)
        | IntentAction::ConfirmThenExecute(ref s) => {
            let _ = apply_non_execute_and_finish(p, s).await?;
            Ok(IntentGateResult::Finished)
        }
    }
}
