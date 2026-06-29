//! 分阶段规划：SSE 与终端通知。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

use crate::sse::{
    SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody, StagedPlanStepFinishedBody,
    StagedPlanStepStartedBody, encode_message,
};

pub(crate) static STAGED_PLAN_SEQ: AtomicU64 = AtomicU64::new(1);

/// 分阶段规划在 **`staged_plan_two_phase_nl_display`** 开启时，于 JSON 规划定稿后追加的无工具 **user** 条正文。
///
/// 首行 [`crate::runtime::plan_section::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX`] 与分步注入同理，聊天区不展示整段。
/// 正文须**明确写清「非用户提问」**，避免模型在思维链里把口语续问误当成用户的原话（曾用「接下来你打算怎么帮我」类问句导致答非所问）。
pub(crate) fn staged_plan_nl_followup_user_body() -> String {
    format!(
        "{}【系统桥接·非用户提问】请只回答对话里**先前真实用户消息**所提的问题（若有附图则含图片说明），并结合已定规划；用两三句自然语言说明你的协助思路即可。勿将本条任何句子当作用户提问来复述、引用或推理。",
        crate::runtime::plan_section::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX
    )
}

/// 内置规划轮 **system** 文案：`no_task` / 空 `steps` 等约定**仅**通过内嵌的 [`crate::agent::plan_artifact::PLAN_V1_SCHEMA_RULES`] 描述，不再追加寒暄类硬提示段落。
pub(crate) fn staged_plan_phase_instruction_default() -> String {
    format!(
        "### 分阶段规划 · 规划轮\n\
         **长度纪律**：若推理/思维链计入本轮完成额度，勿在其中长篇展开；可省略或仅一两句提纲，**优先**尽快给出下方 fenced JSON（`agent_reply_plan`）。附言保持简短。\n\
         请仅根据用户消息做任务拆解，不要调用任何工具，不要执行命令或读写文件。\n\
         **输出形态**：除简短自然语言与 fenced JSON 外，**禁止**输出 DeepSeek/Microsoft 式 `<…DSML…>`、`tool_calls`/`invoke`/`parameter` 等伪 XML 或非 OpenAI 兼容的「伪工具调用」正文；本回合也**不得**使用 API 的 `tool_calls` 字段（否则会被服务端拒绝并重写）。\n\
         **JSON 形状**：顶层须为合法 v1 对象，字段包含 `type`（字符串 `agent_reply_plan`）、`version`、`steps`、`no_task` 等（见下方 schema）；**禁止**把整份计划包在 `\"agent_reply_plan\": {{ ... }}` 嵌套键下。\n\
         `steps` 须与用户意图及粒度一致：用户只要概览、梳理或只读分析时，勿擅自收窄为单一文件的深层修复路径，除非用户明确授权。\n\
         **咨询类**（架构意见、风险列举、优劣对比等）且用户未要求改仓库、新建文档或跑构建/测试：优先 `no_task: true`、`steps: []`，由后续自然语言直接作答；勿拆成多轮「通读大量源文件」或未经要求的长篇设计稿。\n\
         **只读概览**（如「分析当前项目/仓库」、介绍技术栈与目录）：**必须**在 fenced JSON 中设 `no_task: true`、`steps: []`，正文一次性给出结论；**勿**在文末写「如需进一步深入请告诉我方向」等续问句（易触发重复终答）。\n\
         若仍需一步辅助：`steps[0].description` 须写清**可验收结论**（条目化模块/风险点）与**只读探查上限**（如至多 N 个路径/文件）；此类步须 `executor_kind=review_readonly`，勿在未授权步使用 `patch_write`/`test_runner`。\n\
         `steps[].description` 宜具体可验收；用户明确要求多项交付物时勿擅自合并。\n\
         信息不足时宁可 `no_task` 或请求澄清，勿编造路径或接口细节。\n\
         **单步滚动 + 验收**：每轮 JSON 通常仅 **1** 条 `steps`。「先读后写再测」靠**多轮规划**切换 `executor_kind`（`review_readonly` → `patch_write` → `test_runner`），勿在一轮内堆多步（`workflow_validate_only` 绑定等多步例外见下方 schema）。当本步为 **`test_runner`**（如 `cargo_check` / `cargo_test` / 同类）时，除非用户明示无需硬验收，**须在 `acceptance` 中至少给出 `expect_exit_code`，并推荐再加与命令输出一致的短字符串 `expect_stdout_contains` 或 `expect_stderr_contains`**（须为真实 CLI 日志里**会出现**的子串，勿填抽象描述），否则步末难以触发有意义的确定性验收与补丁重规划。\n\
         **`review_readonly` 步**：`acceptance` 仅用于文档/源码等只读路径的 `expect_file_exists`；**禁止**在本步验收可执行二进制（如 `bin/<可执行体>`、`target/release/<crate>`）或 `expect_exit_code`——构建/运行产物须放在后续 `test_runner` 步。解压/归档步用 `expect_file_exists` 验收解压目录，**勿**用 `expect_stdout_contains` 填 `ls` 输出子串。\n\
         **步后滚动**：若对话中已有某 `read_file` / `list_dir` 成功读取目标文档（如 README、INSTALL），**下一轮禁止**再规划同 id 或同描述的 `review_readonly` 读文档步；应推进 `patch_write` / `test_runner`（编译、测试）或 `no_task: true` 总结。\n\
         **步后收敛**：若历史工具输出已经证明用户原始目标完成（例如源码已写入、构建成功、可执行程序已运行且退出码为 0），不要再生成 `verify` / `confirm` / `run-and-confirm` / `verify-run-output` 等等价验证步骤，也不要为了让系统重新识别证据而重复执行同类命令；应返回 `no_task: true`、`steps: []` 或给出简短最终总结。\n\
         正文中须用 Markdown 代码围栏（语言标记为 json）给出合法 JSON，且满足：\n\
         {}\n\
         涉及「先审读→再改→再测」时，为相应步设置 `executor_kind`（`review_readonly` → `patch_write` → `test_runner`）。",
        crate::agent::plan_artifact::PLAN_V1_SCHEMA_RULES,
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
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::ChatUiSeparator { short }),
            "staged_sse::chat_ui_separator",
        )
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
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::StagedPlanNotice { text, clear_before }),
            "staged_sse::staged_plan_notice",
        )
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
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::StagedPlanStarted {
            started: StagedPlanStartedBody {
                plan_id: plan_id.to_string(),
                total_steps,
            },
        }),
        "staged_sse::staged_plan_started",
    )
    .await;
}

pub(crate) async fn send_staged_plan_step_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    description: &str,
    executor_kind: Option<&str>,
) {
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "staged_plan_step_started",
        description,
        Some(&serde_json::json!({
            "plan_id": plan_id,
            "step_id": step_id,
            "step_index": step_index,
            "total_steps": total_steps,
            "description": description,
            "executor_kind": executor_kind,
        })),
    );
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                description: description.to_string(),
                executor_kind: executor_kind.map(|s| s.to_string()),
            },
        }),
        "staged_sse::staged_plan_step_started",
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_staged_plan_step_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
    executor_kind: Option<&str>,
    verify_fail_reason: Option<&str>,
) {
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "staged_plan_step_finished",
        step_id,
        Some(&serde_json::json!({
            "plan_id": plan_id,
            "step_id": step_id,
            "step_index": step_index,
            "total_steps": total_steps,
            "status": status,
            "executor_kind": executor_kind,
            "verify_fail_reason": verify_fail_reason,
        })),
    );
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                status: status.to_string(),
                executor_kind: executor_kind.map(|s| s.to_string()),
                verify_fail_reason: verify_fail_reason.map(|s| s.to_string()),
            },
        }),
        "staged_sse::staged_plan_step_finished",
    )
    .await;
}

/// 发送单步结束 SSE（`failed` / `cancelled` / `ok`）。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finish_staged_plan_step_sse(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id_trim: &str,
    step_index: usize,
    n: usize,
    status: &'static str,
    executor_kind: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    verify_fail_reason: Option<&str>,
) {
    send_staged_plan_step_finished(
        out,
        plan_id,
        step_id_trim,
        step_index,
        n,
        status,
        executor_kind.map(|k| k.as_snake_case_str()),
        verify_fail_reason,
    )
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
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::StagedPlanFinished {
            finished: StagedPlanFinishedBody {
                plan_id: plan_id.to_string(),
                total_steps,
                completed_steps,
                status: status.to_string(),
            },
        }),
        "staged_sse::staged_plan_finished",
    )
    .await;
}

/// 成功收尾：step SSE + 分隔线 + 队列摘要（参数聚合以降低形参棘轮）。
pub(crate) struct StagedStepOkNoticeParams<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub messages: &'a mut Vec<crate::types::Message>,
    pub plan_id: &'a str,
    pub step_id_trim: &'a str,
    pub step_index: usize,
    pub n: usize,
    pub executor_kind: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    pub plan_steps: &'a [crate::agent::plan_artifact::PlanStepV1],
    pub echo_terminal_staged: bool,
}

/// 本步工具链判定成功：发送 `ok` 的 step SSE、分隔线、队列摘要 notice。
pub(crate) async fn staged_step_emit_ok_step_and_queue_notice(p: StagedStepOkNoticeParams<'_>) {
    send_staged_plan_step_finished(
        p.out,
        p.plan_id,
        p.step_id_trim,
        p.step_index,
        p.n,
        "ok",
        p.executor_kind.map(|k| k.as_snake_case_str()),
        None,
    )
    .await;
    p.messages
        .push(crate::types::Message::chat_ui_separator(true));
    let plan_row = crate::agent::plan_artifact::AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: p.plan_steps.to_vec(),
        no_task: false,
    };
    send_staged_plan_notice(
        p.out,
        p.echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan_row, p.step_index),
    )
    .await;
    emit_chat_ui_separator_sse(p.out, true).await;
}
