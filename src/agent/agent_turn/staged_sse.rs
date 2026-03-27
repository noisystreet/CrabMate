//! 分阶段规划：SSE 与终端通知。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

use crate::sse::{
    SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody, StagedPlanStepFinishedBody,
    StagedPlanStepStartedBody, encode_message,
};

pub(crate) static STAGED_PLAN_SEQ: AtomicU64 = AtomicU64::new(1);

/// `allow_no_task`：为 true 时在说明中要求模型在「无具体任务」时输出 `no_task: true` + 空 `steps`，以跳过后续分步注入。
pub(crate) fn staged_plan_phase_instruction_default(allow_no_task: bool) -> String {
    let no_task_hint = if allow_no_task {
        "\n若判断用户**未提出需要执行的具体任务**（如寒暄、致谢、泛泛聊天、仅确认理解等），JSON 中须设 \"no_task\": true，且 \"steps\" 为 []；系统将**不再**分步规划，直接按普通对话继续。"
    } else {
        ""
    };
    format!(
        "【分阶段规划模式 · 规划轮】请仅根据用户消息做任务拆解，不要调用任何工具，不要执行命令或读写文件。\n\
         在回复正文中必须用 Markdown 代码围栏（语言标记为 json）给出一个合法 JSON 对象，且满足：\n\
         {}{}\n\
         可辅以简短自然语言说明；有具体任务时后续系统将按 steps 顺序逐步下发执行指令。",
        crate::agent::plan_artifact::PLAN_V1_SCHEMA_RULES,
        no_task_hint
    )
}

pub(crate) fn staged_plan_queue_summary_text(
    plan: &crate::agent::plan_artifact::AgentReplyPlanV1,
    completed_count: usize,
) -> String {
    let n = plan.steps.len();
    let steps_md = crate::agent::plan_artifact::format_plan_steps_markdown_for_staged_queue(
        plan,
        completed_count,
    );
    let header = format!(
        "{}共 {} 步",
        crate::runtime::plan_section::STAGED_PLAN_SECTION_HEADER,
        n,
    );
    let body = format!("{}\n\n{}", header, steps_md);
    if n > 0 && completed_count >= n {
        format!("[✓] 全部完成\n\n{}", body)
    } else {
        body
    }
}

pub(crate) async fn emit_chat_ui_separator_sse(out: Option<&mpsc::Sender<String>>, short: bool) {
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::ChatUiSeparator { short }))
            .await;
    }
}
pub(crate) async fn send_staged_plan_notice(
    out: Option<&mpsc::Sender<String>>,
    echo_terminal: bool,
    clear_before: bool,
    text: impl Into<String>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    // CLI（`out: None` 且 `render_to_terminal`）无 SSE，把规划/步骤打到 stdout
    if echo_terminal {
        let _ =
            crate::runtime::terminal_cli_transcript::print_staged_plan_notice(clear_before, &text);
    }
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::StagedPlanNotice {
                text,
                clear_before,
            }))
            .await;
    }
}

pub(crate) fn next_staged_plan_id() -> String {
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = STAGED_PLAN_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("staged-{ts_ms}-{seq}")
}

pub(crate) async fn send_staged_plan_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStarted {
            started: StagedPlanStartedBody {
                plan_id: plan_id.to_string(),
                total_steps,
            },
        }))
        .await;
}

pub(crate) async fn send_staged_plan_step_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    description: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                description: description.to_string(),
            },
        }))
        .await;
}

pub(crate) async fn send_staged_plan_step_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                status: status.to_string(),
            },
        }))
        .await;
}

pub(crate) async fn send_staged_plan_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
    completed_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanFinished {
            finished: StagedPlanFinishedBody {
                plan_id: plan_id.to_string(),
                total_steps,
                completed_steps,
                status: status.to_string(),
            },
        }))
        .await;
}
