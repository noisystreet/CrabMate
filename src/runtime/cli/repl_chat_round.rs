//! REPL 普通对话回合：合并上下文、校验密钥、跑一轮 agent。

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::ProcessHandles;
use crate::clarification_questionnaire::merge_user_text_with_clarification_answers;
use crate::config::{LlmHttpAuthMode, SharedAgentConfig};
use crate::redact;
use crate::runtime::cli::chat::{RunAgentTurnForCliParams, run_agent_turn_for_cli};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::tool_registry::CliToolRuntime;
use crate::types::Message;
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use log::debug;

/// TUI 等：用户句已入队 `messages` 后的即时刷新回调（不等整轮模型返回）。
pub(crate) type ReplAfterUserMessageEnqueuedCb = Arc<dyn Fn(&[Message]) + Send + Sync>;

/// 普通对话回合所需句柄（不含 `/` 斜杠命令分支）。
pub(crate) struct ReplDispatchChatRoundParams<'a> {
    pub(crate) input: String,
    pub(crate) cfg_holder: &'a SharedAgentConfig,
    pub(crate) tools: &'a [crate::types::Tool],
    pub(crate) messages: &'a mut Vec<Message>,
    pub(crate) work_dir: &'a mut Path,
    pub(crate) style: &'a CliReplStyle,
    pub(crate) no_stream: bool,
    pub(crate) suppress_stdout_render: bool,
    pub(crate) tui_llm_stream_scratch: Option<crabmate_llm::TuiLlmStreamScratchArc>,
    /// TUI：工具批开始/结束回调（底栏「工具执行中…」）；REPL 为 `None`。
    pub(crate) tool_running_hook: Option<std::sync::Arc<dyn Fn(bool) + Send + Sync>>,
    /// TUI 等：用户消息已写入 `messages` 后立即刷新展示（不等整轮 `run_agent_turn` 结束）。
    pub(crate) after_user_message_enqueued: Option<ReplAfterUserMessageEnqueuedCb>,
    pub(crate) agent_role_owned: &'a mut Option<String>,
    pub(crate) api_key_holder: &'a Arc<StdMutex<String>>,
    pub(crate) client: &'a reqwest::Client,
    pub(crate) cli_rt: &'a CliToolRuntime,
    pub(crate) initial_pending: Option<&'a Arc<StdMutex<Option<Vec<crate::types::Message>>>>>,
    pub(crate) process_handles: Arc<ProcessHandles>,
    /// TUI：问卷 Modal 提交后在下一轮并入用户正文（与 Web `clarify_questionnaire_answers` 对齐）。
    pub(crate) clarify_answers_for_next_user_message: Option<
        &'a Arc<StdMutex<Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>>>,
    >,
    /// TUI：`present_clarification_questionnaire` 回调；`repl` 为 `None`。
    pub(crate) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    /// TUI：SSE 控制面镜像；`repl` 为 `None`。
    pub(crate) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 与 `/mode` 共用的会话工作模式。
    pub(crate) session_mode: &'a Arc<StdMutex<crate::types::SessionMode>>,
}

struct ReplRefreshFirstSystemParams<'a> {
    cfg_holder: &'a SharedAgentConfig,
    messages: &'a mut [Message],
    work_dir: &'a Path,
    agent_role_owned: &'a Option<String>,
    user_body: &'a str,
    forced_skill: Option<crate::config::skills::SkillDoc>,
    session_mode_now: crate::types::SessionMode,
    process_handles: &'a ProcessHandles,
    style: &'a CliReplStyle,
}

async fn repl_refresh_first_system_for_turn(p: ReplRefreshFirstSystemParams<'_>) -> Result<(), ()> {
    let ReplRefreshFirstSystemParams {
        cfg_holder,
        messages,
        work_dir,
        agent_role_owned,
        user_body,
        forced_skill,
        session_mode_now,
        process_handles,
        style,
    } = p;
    let g = cfg_holder.read().await;
    let Some(first) = messages.first_mut() else {
        return Ok(());
    };
    if first.role != "system" {
        return Ok(());
    }
    let role = match crate::context_bootstrap::prompt_compose::resolve_agent_role_for_prompt_compose(
        &g,
        None,
        agent_role_owned.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            let _ = style.eprint_error(&e);
            return Err(());
        }
    };
    let (merged, diag) =
        match crate::context_bootstrap::prompt_compose::compose_first_system_for_turn_with_diagnostics(
            &g,
            &process_handles.tool_outcome_recorder,
            crate::context_bootstrap::prompt_compose::FirstSystemComposeOpts {
                agent_role: role.as_deref(),
                user_msg_for_skills: Some(user_body),
                skills_base_dir: Some(work_dir.to_path_buf()),
                forced_skill,
                role_resolution:
                    crate::context_bootstrap::prompt_compose::RoleSystemResolution::Strict,
                session_mode: Some(session_mode_now),
            },
        ) {
            Ok(v) => v,
            Err(e) => {
                let _ = style.eprint_error(&e);
                return Err(());
            }
        };
    debug!(
        target: "crabmate",
        "first_system_compose path=repl_turn_refresh layers={:?} chars_l3={} chars_l4={} chars_final={} skills_total={} skills_selected={:?}",
        diag.layers_applied,
        diag.chars_l3_base,
        diag.chars_l4_augmented,
        diag.chars_final,
        diag.skills_total_docs,
        diag.skills_selected_labels
    );
    first.content = Some(crate::types::MessageContent::Text(merged));
    Ok(())
}

async fn repl_block_if_bearer_missing_api_key(
    cfg_holder: &SharedAgentConfig,
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> bool {
    let g = cfg_holder.read().await;
    if g.llm.llm_http_auth_mode != LlmHttpAuthMode::Bearer {
        return true;
    }
    let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
    if !k.trim().is_empty() {
        return true;
    }
    drop(k);
    let _ = style.eprint_error(
        "当前为 llm_http_auth_mode=bearer，但未配置 LLM API 密钥。请执行 /api-key set <密钥>（仅本进程）、设置环境变量 API_KEY，或在 Web/桌面侧栏保存到系统钥匙串后重启。",
    );
    false
}

pub(crate) async fn repl_dispatch_chat_round(
    p: ReplDispatchChatRoundParams<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ReplDispatchChatRoundParams {
        input,
        cfg_holder,
        tools,
        messages,
        work_dir,
        style,
        no_stream,
        suppress_stdout_render,
        tui_llm_stream_scratch,
        tool_running_hook,
        after_user_message_enqueued,
        agent_role_owned,
        api_key_holder,
        client,
        cli_rt,
        initial_pending,
        process_handles,
        clarify_answers_for_next_user_message,
        clarification_questionnaire_hook,
        sse_control_mirror,
        session_mode,
    } = p;
    let session_mode_now = *session_mode.lock().unwrap_or_else(|e| e.into_inner());
    crate::runtime::workspace_session::try_merge_background_initial_workspace(
        messages,
        initial_pending,
    );
    let expanded_user = {
        let g = cfg_holder.read().await;
        match expand_at_file_refs_in_user_message(input.as_str(), work_dir, &g) {
            Ok(s) => s,
            Err(e) => {
                let _ = style.eprint_error(&e);
                return Ok(());
            }
        }
    };
    let clarify_take = clarify_answers_for_next_user_message
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));
    let user_body = merge_user_text_with_clarification_answers(expanded_user, clarify_take);
    let prepared = {
        let g = cfg_holder.read().await;
        match crate::config::skills_slash::prepare_user_message_for_skills(
            &user_body,
            g.skills.list_opts(work_dir),
            g.skills.skills_enabled,
        ) {
            Ok(p) => p,
            Err(e) => {
                let _ = style.eprint_error(&e.to_string());
                return Ok(());
            }
        }
    };
    let user_body = prepared.user_message;
    if repl_refresh_first_system_for_turn(ReplRefreshFirstSystemParams {
        cfg_holder,
        messages,
        work_dir,
        agent_role_owned,
        user_body: user_body.as_str(),
        forced_skill: prepared.forced_skill,
        session_mode_now,
        process_handles: &process_handles,
        style,
    })
    .await
    .is_err()
    {
        return Ok(());
    }
    messages.push(Message::user_only(user_body));
    if let Some(cb) = after_user_message_enqueued.as_ref() {
        cb(messages.as_slice());
    }
    debug!(
        target: "crabmate::print",
        "REPL 用户输入已入队 history_len={} input_preview={}",
        messages.len(),
        redact::preview_chars(input.as_str(), redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    // 须在入队用户消息之后再拦截：否则 TUI 等仅依赖 `messages` 的界面看不到已发送输入。
    if !repl_block_if_bearer_missing_api_key(cfg_holder, api_key_holder, style).await {
        return Ok(());
    }

    let cfg_snap = {
        let g = cfg_holder.read().await;
        Arc::new(g.clone())
    };
    let key_snap = api_key_holder
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Err(e) = run_agent_turn_for_cli(RunAgentTurnForCliParams {
        client,
        api_key: key_snap.as_str(),
        cfg: &cfg_snap,
        tools,
        messages,
        work_dir,
        no_stream,
        suppress_stdout_render,
        tui_llm_stream_scratch,
        tool_running_hook,
        clarification_questionnaire_hook,
        cli_tool_ctx: Some(cli_rt),
        active_agent_role: agent_role_owned.as_deref(),
        process_handles: Arc::clone(&process_handles),
        sse_control_mirror,
        session_mode: session_mode_now,
    })
    .await
    {
        let _ = style.eprint_error(&format!(
            "本轮对话失败（可继续输入；异常历史可 /clear 清空）：{}",
            e
        ));
    }
    Ok(())
}
