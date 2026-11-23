//! 首轮/续轮消息组装、SSE 协议与流式请求解析（与 `chat` handler 共用的准备逻辑）。

use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use tracing::debug;

use super::super::super::app_state::ConversationTurnSeed;
use super::super::parse::{
    normalize_agent_role, normalize_chat_image_urls, normalize_client_conversation_id,
    parse_client_llm_override, parse_executor_llm_override, parse_optional_chat_temperature,
    parse_readonly_tool_ttl_cache_secs, parse_seed_override_from_body,
};
use crate::agent_role_turn::maybe_apply_mid_session_agent_role_switch;
use crate::chat_job_queue;
use crate::clarification_questionnaire::normalize_clarify_questionnaire_answers_raw;
use crate::context_bootstrap::conversation_turn_bootstrap::{
    compose_new_conversation_messages, first_turn_project_context_user_message_for_web,
};
use crate::memory::agent_memory::load_memory_snippet;
use crate::types::Message;
use crate::web::app_state_facets::WebChatTurnAppFacet;
use crate::web::http_types::chat::{ApiError, ChatRequestBody, StreamResumeBody};
use crate::web::http_types::validation::validate_chat_request_payload_limits;

use crate::context_bootstrap::prompt_compose::{
    FirstSystemComposeOpts, RoleSystemResolution, compose_first_system_for_turn_with_diagnostics,
    resolve_skills_base_dir,
};

type ChatPayloadError = (StatusCode, Json<ApiError>);

#[inline]
pub(super) fn bad_request(code: &'static str, message: impl Into<String>) -> ChatPayloadError {
    (StatusCode::BAD_REQUEST, Json(ApiError::new(code, message)))
}

pub(crate) fn reject_if_client_sse_protocol_invalid(
    client_sse_protocol: Option<u8>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(v) = client_sse_protocol else {
        return Ok(());
    };
    let supported = crate::sse::protocol::SSE_PROTOCOL_VERSION;
    if v == 0 {
        return Err(bad_request(
            crate::cm_api_contract::error_codes::INVALID_SSE_CLIENT_PROTOCOL,
            "client_sse_protocol 不能为 0",
        ));
    }
    if v > supported {
        return Err(bad_request(
            crate::cm_api_contract::error_codes::SSE_CLIENT_TOO_NEW,
            format!("客户端 SSE 协议版本 v{v} 高于服务端 v{supported}"),
        ));
    }
    if v < supported {
        return Err(bad_request(
            crate::cm_api_contract::error_codes::SSE_PROTOCOL_MISMATCH,
            format!("仅支持 SSE 协议版本 {supported}（收到 v{v}）"),
        ));
    }
    Ok(())
}

pub(super) fn parse_last_event_id(headers: &axum::http::HeaderMap) -> Option<u64> {
    let raw = headers.get(axum::http::HeaderName::from_static("last-event-id"))?;
    let s = raw.to_str().ok()?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

/// 解析 `session_mode` 与 `permission_level` 两个「本回合覆盖」字段。
///
/// 合并成一个 helper 的两个理由：两者都是可选的会话级覆盖、带着同一套 `INVALID_*` 错误码；
/// 而 `parse_chat_*_request_tail` 的圈复杂度已贴着仓库上限（CCN ≤ 10），
/// 再在里面各加一个 `?` 就要拆函数，收益不如就地收敛。
pub(super) fn parse_chat_mode_and_permission_level(
    body: &ChatRequestBody,
) -> Result<(Option<String>, Option<String>), ChatPayloadError> {
    let session_mode = crate::types::parse_optional_session_mode(body.session_mode.as_deref())
        .map_err(|e| bad_request("INVALID_SESSION_MODE", e))?
        .map(|m| m.as_str().to_string());
    let permission_level =
        crate::cm_types::parse_optional_permission_level(body.permission_level.as_deref())
            .map_err(|e| bad_request("INVALID_PERMISSION_LEVEL", e))?
            .map(|l| l.as_str().to_string());
    Ok((session_mode, permission_level))
}

/// `chat_stream_handler` 前半段：校验并解析请求体（降低主 handler 圈复杂度）。
pub(super) struct ChatStreamRequestParsed {
    pub(super) resume: Option<StreamResumeBody>,
    pub(super) image_urls: Vec<String>,
    pub(super) clarify: Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
    pub(super) user_trim: String,
    pub(super) conversation_id: String,
    pub(super) agent_role: Option<String>,
    pub(super) session_mode: Option<String>,
    /// 本请求显式申请的权限等级（已校验为合法串）；`None` = 沿用会话持久值。
    pub(super) permission_level: Option<String>,
    pub(super) temperature_override: Option<f32>,
    pub(super) seed_override: crate::types::LlmSeedOverride,
    pub(super) client_sse_protocol: Option<u8>,
    pub(super) llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(super) executor_llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(super) readonly_tool_ttl_cache_secs: Option<u64>,
}

pub(super) fn parse_chat_stream_request(
    state: &WebChatTurnAppFacet,
    body: &ChatRequestBody,
) -> Result<ChatStreamRequestParsed, ChatPayloadError> {
    validate_chat_request_payload_limits(body)?;
    let resume = body.stream_resume.clone();
    let image_urls = normalize_chat_image_urls(&body.image_urls)
        .map_err(|e| bad_request("INVALID_IMAGE_URLS", e))?;
    let clarify = if let Some(ref c) = body.clarify_questionnaire_answers {
        normalize_clarify_questionnaire_answers_raw(c.questionnaire_id.clone(), c.answers.clone())
            .map_err(|e| bad_request("INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS", e))?
    } else {
        None
    };
    let user_trim = body.message.trim().to_string();
    if user_trim.is_empty() && resume.is_none() && image_urls.is_empty() && clarify.is_none() {
        return Err(bad_request(
            "EMPTY_MESSAGE",
            "提问内容不能为空（若仅发图须至少附带一张图片；澄清问卷作答可单独提交）",
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    parse_chat_stream_request_tail(state, body, resume, image_urls, clarify, user_trim)
}

fn parse_chat_stream_request_tail(
    state: &WebChatTurnAppFacet,
    body: &ChatRequestBody,
    resume: Option<StreamResumeBody>,
    image_urls: Vec<String>,
    clarify: Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
    user_trim: String,
) -> Result<ChatStreamRequestParsed, ChatPayloadError> {
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| bad_request("INVALID_CONVERSATION_ID", e))?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref())
        .map_err(|e| bad_request("INVALID_AGENT_ROLE", e))?;
    let (session_mode, permission_level) = parse_chat_mode_and_permission_level(body)?;
    let temperature_override = parse_optional_chat_temperature(body.temperature)
        .map_err(|e| bad_request("INVALID_TEMPERATURE", e))?;
    let seed_override = parse_seed_override_from_body(body.seed, body.seed_policy.clone())
        .map_err(|e| bad_request("INVALID_SEED", e))?;
    let llm_override = parse_client_llm_override(crate::user_data::merge_client_llm_body(
        body.client_llm.clone(),
    ))
    .map_err(|e| bad_request("INVALID_CLIENT_LLM", e))?;
    let executor_llm_override = parse_executor_llm_override(
        crate::user_data::merge_executor_llm_body(body.executor_llm.clone()),
    )
    .map_err(|e| bad_request("INVALID_EXECUTOR_LLM", e))?;
    let readonly_tool_ttl_cache_secs =
        parse_readonly_tool_ttl_cache_secs(body.readonly_tool_ttl_cache_secs)
            .map_err(|e| bad_request("INVALID_READONLY_TOOL_TTL_CACHE_SECS", e))?;
    Ok(ChatStreamRequestParsed {
        resume,
        image_urls,
        clarify,
        user_trim,
        conversation_id,
        agent_role,
        session_mode,
        permission_level,
        temperature_override,
        seed_override,
        client_sse_protocol: body.client_sse_protocol,
        llm_override,
        executor_llm_override,
        readonly_tool_ttl_cache_secs,
    })
}

fn prepare_turn_user_and_forced_skill(
    cfg: &crate::config::AgentConfig,
    workspace_root: &std::path::Path,
    user_msg: &str,
) -> Result<(String, Option<crate::config::skills::SkillDoc>), String> {
    let skills_base = resolve_skills_base_dir(workspace_root);
    let prepared = crate::config::skills_slash::prepare_user_message_for_skills(
        user_msg,
        cfg.skills.list_opts(skills_base.as_path()),
        cfg.skills.skills_enabled,
    )
    .map_err(crate::config::skills_slash::SkillSlashError::into_turn_err)?;
    Ok((prepared.user_message, prepared.forced_skill))
}

/// 本回合 `/<id>` 强制技能正文的 user 侧注入条（`None` = 无强制技能或技能未启用）。
///
/// 放在真实 user 之后而**不是**首条 `system` 里：强制正文只在本回合有效，放进 system
/// 会让该回合整段前缀缓存失效（#1475）。该注入登记为运行时刷新型，落盘前剥离。
fn forced_skill_injection_message(
    cfg: &crate::config::AgentConfig,
    forced_skill: Option<&crate::config::skills::SkillDoc>,
) -> Option<crate::types::Message> {
    let doc = forced_skill.filter(|_| cfg.skills.skills_enabled)?;
    let appendix =
        crate::config::skills::forced_skill_appendix_for_injection(doc, cfg.skills.skills_max_chars);
    if appendix.trim().is_empty() {
        return None;
    }
    Some(crate::types::Message::user_forced_skill_injection(appendix))
}

/// 新会话首轮 `messages`：`system` + 可选首轮上下文 + 真实 user + 可选 `/<id>` 注入。
///
/// 抽成纯函数是为了让**顺序不变量**可单测（注入必须紧跟真实 user，且不得回到 `system`）：
/// `build_messages_for_turn` 需要完整的 `WebChatTurnAppFacet`（conversation / chat runtime、
/// SSE hub 等十余个字段），直接构造来测顺序代价过高且易碎（#1475）。
pub(super) fn compose_new_conversation_turn_messages(
    system_for_turn: &str,
    project_context: Option<String>,
    last_user: crate::types::Message,
    forced_skill_injection: Option<crate::types::Message>,
) -> Vec<crate::types::Message> {
    let mut messages =
        compose_new_conversation_messages(system_for_turn, project_context, Some(last_user));
    if let Some(m) = forced_skill_injection {
        messages.push(m);
    }
    messages
}

/// 续聊：把真实 user 与可选 `/<id>` 注入按固定顺序追加到既有历史末尾。
pub(super) fn append_turn_user_and_forced_skill(
    messages: &mut Vec<crate::types::Message>,
    last_user: crate::types::Message,
    forced_skill_injection: Option<crate::types::Message>,
) {
    messages.push(last_user);
    if let Some(m) = forced_skill_injection {
        messages.push(m);
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_existing_turn_system(
    cfg: &crate::config::AgentConfig,
    seed: &mut ConversationTurnSeed,
    agent_role: Option<&str>,
    session_mode: Option<crate::types::SessionMode>,
    workspace_root: &std::path::Path,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    forced_skill: Option<&crate::config::skills::SkillDoc>,
) -> Result<(), String> {
    let persisted = seed.persisted_active_agent_role.clone();
    if let Some(id) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
        cfg.system_prompt_for_new_conversation(Some(id))
            .map_err(|e| e.to_string())?;
    }
    let mode = crate::session_mode_turn::resolve_session_mode_for_turn(
        session_mode.map(|m| m.as_str()),
        seed.persisted_active_session_mode.as_deref(),
        crate::session_mode_turn::role_default_session_mode_for_turn(
            cfg,
            persisted.as_deref(),
            agent_role,
        ),
        cfg.roles_prompts.default_session_mode,
    )?;
    maybe_apply_mid_session_agent_role_switch(
        cfg,
        &mut seed.messages,
        persisted.as_deref(),
        agent_role,
        tool_recorder,
        Some(workspace_root),
        Some(mode),
    )?;
    let role_for_turn =
        crate::context_bootstrap::prompt_compose::resolve_agent_role_for_prompt_compose(
            cfg,
            agent_role,
            persisted.as_deref(),
        )?;
    let skills_base = resolve_skills_base_dir(workspace_root);
    let (system_for_turn, diag) = compose_first_system_for_turn_with_diagnostics(
        cfg,
        tool_recorder,
        FirstSystemComposeOpts {
            agent_role: role_for_turn.as_deref(),
            skills_base_dir: Some(skills_base),
            workspace_root: Some(workspace_root.to_path_buf()),
            forced_skill: forced_skill.cloned(),
            role_resolution: RoleSystemResolution::Strict,
            session_mode: Some(mode),
        },
    )?;
    debug!(
        target: "crabmate",
        "first_system_compose path=web_existing conversation_id_refresh=true layers={:?} chars_l3={} chars_l4={} chars_final={} skills_total={} skills_selected={:?}",
        diag.layers_applied,
        diag.chars_l3_base,
        diag.chars_l4_augmented,
        diag.chars_final,
        diag.skills_total_docs,
        diag.skills_selected_labels
    );
    if let Some(first) = seed.messages.first_mut()
        && first.role == "system"
    {
        first.content = Some(crate::types::MessageContent::Text(system_for_turn));
    }
    Ok(())
}

struct TurnBuildParts<'a> {
    agent_role: Option<&'a str>,
    mode_opt: Option<crate::types::SessionMode>,
    /// 本请求显式申请的权限等级；`None` = 沿用 seed 里的持久意图。
    permission_level: Option<&'a str>,
    root: &'a std::path::Path,
    forced_skill: Option<crate::config::skills::SkillDoc>,
    last_user: Message,
}

async fn build_existing_conversation_turn(
    state: &WebChatTurnAppFacet,
    mut seed: ConversationTurnSeed,
    parts: TurnBuildParts<'_>,
) -> Result<ConversationTurnSeed, String> {
    let forced_skill_injection = {
        let cfg = state.cfg.read().await;
        refresh_existing_turn_system(
            &cfg,
            &mut seed,
            parts.agent_role,
            parts.mode_opt,
            parts.root,
            &state.process_handles.tool_outcome_recorder,
            parts.forced_skill.as_ref(),
        )?;
        forced_skill_injection_message(&cfg, parts.forced_skill.as_ref())
    };
    if let Some(m) = parts.mode_opt {
        seed.persisted_active_session_mode = Some(m.as_str().to_string());
    }
    // 权限等级与 session_mode 同一条取向：请求显式给了就覆盖本会话的持久意图，
    // 没给就原样保留（回合结束后 worker 也按同样的规则落盘）。
    if let Some(level) = parts.permission_level {
        seed.persisted_active_permission_level = Some(level.to_string());
    }
    // `/<id>` 正文紧跟真实 user：system 与既有历史因此保持逐字节稳定（#1475）。
    append_turn_user_and_forced_skill(
        &mut seed.messages,
        parts.last_user,
        forced_skill_injection,
    );
    Ok(seed)
}

async fn build_new_conversation_turn(
    state: &WebChatTurnAppFacet,
    parts: TurnBuildParts<'_>,
) -> Result<ConversationTurnSeed, String> {
    let cfg = state.cfg.read().await;
    let role_for_turn =
        crate::context_bootstrap::prompt_compose::resolve_agent_role_for_prompt_compose(
            &cfg,
            parts.agent_role,
            None,
        )?;
    let mode = crate::session_mode_turn::resolve_session_mode_for_turn(
        parts.mode_opt.map(|m| m.as_str()),
        None,
        crate::session_mode_turn::role_default_session_mode_for_turn(&cfg, None, parts.agent_role),
        cfg.roles_prompts.default_session_mode,
    )?;
    let skills_base = resolve_skills_base_dir(parts.root);
    // 先算出 user 侧注入（需要 `SkillDoc`），再把所有权交给 system 组装（那里只用诊断标签）。
    let forced_skill_injection = forced_skill_injection_message(&cfg, parts.forced_skill.as_ref());
    let (system_for_turn, diag) = compose_first_system_for_turn_with_diagnostics(
        &cfg,
        &state.process_handles.tool_outcome_recorder,
        FirstSystemComposeOpts {
            agent_role: role_for_turn.as_deref(),
            skills_base_dir: Some(skills_base),
            workspace_root: Some(parts.root.to_path_buf()),
            forced_skill: parts.forced_skill,
            role_resolution: RoleSystemResolution::Strict,
            session_mode: Some(mode),
        },
    )?;
    debug!(
        target: "crabmate",
        "first_system_compose path=web_new conversation_id_refresh=false layers={:?} chars_l3={} chars_l4={} chars_final={} skills_total={} skills_selected={:?}",
        diag.layers_applied,
        diag.chars_l3_base,
        diag.chars_l4_augmented,
        diag.chars_final,
        diag.skills_total_docs,
        diag.skills_selected_labels
    );
    let memory_snippet = if cfg.context_bootstrap_inject.agent_memory_file_enabled {
        load_memory_snippet(
            parts.root,
            cfg.context_bootstrap_inject.agent_memory_file.as_str(),
            cfg.context_bootstrap_inject.agent_memory_file_max_chars,
        )
    } else {
        None
    };

    let combined =
        first_turn_project_context_user_message_for_web(parts.root, &cfg, memory_snippet).await;
    let messages = compose_new_conversation_turn_messages(
        &system_for_turn,
        combined,
        parts.last_user,
        forced_skill_injection,
    );
    Ok(ConversationTurnSeed {
        messages,
        expected_revision: None,
        persisted_active_agent_role: None,
        persisted_active_session_mode: Some(mode.as_str().to_string()),
        persisted_active_permission_level: parts.permission_level.map(str::to_string),
        layout: None,
    })
}

/// `build_messages_for_turn` 的失败原因。
///
/// 三个分支各自对应一个 HTTP `code`，调用方**按变体**选错误码；以前是拿
/// `e.contains("session_mode")` 反推，文案一改就静默落到 `INVALID_AGENT_ROLE`。
#[derive(Debug)]
pub(super) enum TurnBuildError {
    /// `/<skill-id>` 技能调用失败（`TURN_ERR_TAG` 已在构造处剥掉）。
    Skill(String),
    /// `session_mode` 非法（来自 `parse_optional_session_mode`）。
    SessionMode(String),
    /// 其余（角色、会话种子、上下文组装等）。
    Other(String),
}

impl TurnBuildError {
    /// 技能错误靠 `SkillSlashError::strip_turn_err` 判定（结构化前缀），不靠猜文案。
    fn from_turn_prepare(raw: String) -> Self {
        match crate::config::skills_slash::SkillSlashError::strip_turn_err(&raw) {
            Some(msg) => Self::Skill(msg.to_string()),
            None => Self::Other(raw),
        }
    }

    /// 契约错误码（常量见 `cm_api_contract::error_codes`）。
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::Skill(_) => crate::cm_api_contract::error_codes::SKILL_INVOKE_FAILED,
            Self::SessionMode(_) => crate::cm_api_contract::error_codes::INVALID_SESSION_MODE,
            // `Other` 实际还涵盖会话种子 / 上下文组装失败，但回包仍沿用
            // `INVALID_AGENT_ROLE`：这是 `POST /chat*` 既有契约（客户端可能按此码提示
            // 「角色不存在」），本轮只把判定从文案改成变体，**不**顺手改码。细分值得另开 Issue。
            Self::Other(_) => crate::cm_api_contract::error_codes::INVALID_AGENT_ROLE,
        }
    }

    pub(super) fn message(&self) -> &str {
        match self {
            Self::Skill(m) | Self::SessionMode(m) | Self::Other(m) => m,
        }
    }

    pub(super) fn into_message(self) -> String {
        match self {
            Self::Skill(m) | Self::SessionMode(m) | Self::Other(m) => m,
        }
    }
}

pub(super) async fn build_messages_for_turn(
    state: &WebChatTurnAppFacet,
    conversation_id: &str,
    user_msg: &str,
    image_urls: &[String],
    agent_role: Option<&str>,
    session_mode: Option<&str>,
    permission_level: Option<&str>,
) -> Result<ConversationTurnSeed, TurnBuildError> {
    let root_str = state.effective_workspace_path().await;
    let root = std::path::PathBuf::from(root_str);
    // 存盘 / 水合 / UI：`last_user` 保留含 `/<skill-id>` 的原文（及上游已展开的 file 附录）。
    // 剥离 `/<skill-id>` 只为检出 forced skill：正文由 `/<id>` 强制注入，其余条文走 `skill_read` 按需读取
    // （旧的按轮打分预注入已删除，#1326）；出站 `messages_for_api_*` 会对普通 user 再剥一次，多轮历史也不带斜杠回供应商。
    let raw_user_for_store = user_msg.to_string();
    let (_stripped_user, forced_skill) = {
        let cfg = state.cfg.read().await;
        prepare_turn_user_and_forced_skill(&cfg, root.as_path(), user_msg)
            .map_err(TurnBuildError::from_turn_prepare)?
    };
    let last_user = if image_urls.is_empty() {
        Message::user_only(raw_user_for_store)
    } else {
        Message::user_with_images(&raw_user_for_store, image_urls)
    };
    let mode_opt = crate::types::parse_optional_session_mode(session_mode)
        .map_err(TurnBuildError::SessionMode)?;
    let parts = TurnBuildParts {
        agent_role,
        mode_opt,
        permission_level,
        root: root.as_path(),
        forced_skill,
        last_user,
    };
    if let Some(seed) = state.load_conversation_seed(conversation_id).await {
        return build_existing_conversation_turn(state, seed, parts)
            .await
            .map_err(TurnBuildError::Other);
    }
    build_new_conversation_turn(state, parts)
        .await
        .map_err(TurnBuildError::Other)
}

#[cfg(test)]
mod client_sse_protocol_tests {
    use super::reject_if_client_sse_protocol_invalid;
    use crate::cm_api_contract::error_codes;

    #[test]
    fn omits_ok() {
        assert!(reject_if_client_sse_protocol_invalid(None).is_ok());
    }

    #[test]
    fn matching_version_ok() {
        let v = crate::sse::protocol::SSE_PROTOCOL_VERSION;
        assert!(reject_if_client_sse_protocol_invalid(Some(v)).is_ok());
    }

    #[test]
    fn zero_is_invalid() {
        let err = reject_if_client_sse_protocol_invalid(Some(0)).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.1.0.code, error_codes::INVALID_SSE_CLIENT_PROTOCOL);
    }

    #[test]
    fn newer_client_rejected() {
        let v = crate::sse::protocol::SSE_PROTOCOL_VERSION.saturating_add(1);
        let err = reject_if_client_sse_protocol_invalid(Some(v)).unwrap_err();
        assert_eq!(err.1.0.code, error_codes::SSE_CLIENT_TOO_NEW);
    }

    #[test]
    fn older_declared_version_mismatch() {
        let supported = crate::sse::protocol::SSE_PROTOCOL_VERSION;
        if supported <= 1 {
            return;
        }
        let err = reject_if_client_sse_protocol_invalid(Some(supported - 1)).unwrap_err();
        assert_eq!(err.1.0.code, error_codes::SSE_PROTOCOL_MISMATCH);
    }
}

/// `parse_chat_mode_and_permission_level` 的解析与错误码（S5b 的写入口）。
///
/// 未知值必须 400 `INVALID_PERMISSION_LEVEL`——静默回落到配置默认，等于把客户端的
/// 显式选择当成没发生，而这正是「切了 L4 却没反应」那类困惑的来源。
#[cfg(test)]
mod chat_mode_and_permission_level_tests {
    use super::parse_chat_mode_and_permission_level;
    use crate::cm_api_contract::chat::{ChatRequestBody, ChatRequestBodyWire};

    fn body(permission_level: Option<&str>) -> ChatRequestBody {
        let mut wire: ChatRequestBodyWire =
            serde_json::from_value(serde_json::json!({ "message": "hi" })).expect("wire");
        wire.permission_level = permission_level.map(str::to_string);
        ChatRequestBody::from(wire)
    }

    #[test]
    fn accepts_a_known_level() {
        let Ok((mode, level)) = parse_chat_mode_and_permission_level(&body(Some("read"))) else {
            panic!("合法等级不应报错")
        };
        assert_eq!(mode, None);
        assert_eq!(level.as_deref(), Some("read"));
    }

    #[test]
    fn absent_means_unset() {
        let Ok((_, level)) = parse_chat_mode_and_permission_level(&body(None)) else {
            panic!("省略等级不应报错")
        };
        assert_eq!(level, None);
    }

    #[test]
    fn unknown_level_is_invalid_permission_level() {
        let err = parse_chat_mode_and_permission_level(&body(Some("root"))).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.1.0.code, "INVALID_PERMISSION_LEVEL");
    }

    #[test]
    fn session_mode_and_level_are_parsed_together() {
        let mut b = body(Some("workspace"));
        b.session_mode = Some("ask".to_string());
        let Ok((mode, level)) = parse_chat_mode_and_permission_level(&b) else {
            panic!("两个字段同时合法不应报错")
        };
        assert_eq!(mode.as_deref(), Some("ask"));
        assert_eq!(level.as_deref(), Some("workspace"));
    }
}

/// `/<id>` 正文的**注入位置**不变量（#1475）：必须紧跟真实 user，且不得回到 `system`。
///
/// 只测顺序与门控，不构造 `WebChatTurnAppFacet`——那需要 conversation / chat runtime 等
/// 十余个字段，测的也只是 wiring 而非逻辑。
#[cfg(test)]
mod forced_skill_injection_order_tests {
    use super::*;
    use crate::cm_config::skills::SkillDoc;
    use crate::cm_types::{CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_FORCED_SKILL_NAME};
    use crate::types::{Message, MessageContent};

    fn alpha_doc() -> SkillDoc {
        SkillDoc {
            display_path: "skills/alpha.md".to_string(),
            content: "ALPHA_BODY_ONLY".to_string(),
            name: Some("alpha".to_string()),
            description: Some("Alpha desc".to_string()),
        }
    }

    fn cfg_with_skills(enabled: bool) -> crate::config::AgentConfig {
        let mut cfg = crate::cm_config::load_config_test_env::without_cm_env_overrides(|| {
            crate::cm_config::load_config(None).expect("embed default")
        });
        cfg.skills.skills_enabled = enabled;
        cfg.skills.skills_max_chars = 4000;
        cfg
    }

    fn text_of(m: &Message) -> String {
        match &m.content {
            Some(MessageContent::Text(t)) => t.clone(),
            other => panic!("期望文本正文，实际 {other:?}"),
        }
    }

    /// 只在「技能启用 + 本轮有 `/<id>`」时才产生注入条。
    #[test]
    fn injection_message_only_when_skill_enabled_and_forced() {
        let on = cfg_with_skills(true);
        let m = forced_skill_injection_message(&on, Some(&alpha_doc())).expect("应有注入条");
        assert_eq!(m.role, "user");
        assert_eq!(m.name.as_deref(), Some(CRABMATE_FORCED_SKILL_NAME));
        assert!(text_of(&m).contains("ALPHA_BODY_ONLY"));

        let off = cfg_with_skills(false);
        assert!(
            forced_skill_injection_message(&off, Some(&alpha_doc())).is_none(),
            "skills 关闭时不应注入"
        );
        assert!(
            forced_skill_injection_message(&on, None).is_none(),
            "无 /<id> 时不应注入"
        );
    }

    /// 新会话：注入排在真实 user **之后**，且 system 不含正文。
    #[test]
    fn new_conversation_injects_after_real_user_not_into_system() {
        let messages = compose_new_conversation_turn_messages(
            "SYSTEM_BASE",
            Some("FIRST_TURN_CTX".to_string()),
            Message::user_only("真实问题"),
            Some(Message::user_forced_skill_injection("ALPHA_BODY_ONLY")),
        );

        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["system", "user", "user", "user"]);
        assert_eq!(
            messages[1].name.as_deref(),
            Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME),
            "第二条应是首轮工作区上下文"
        );
        assert_eq!(messages[2].name, None, "倒数第二条必须是真实 user");
        assert_eq!(
            messages[3].name.as_deref(),
            Some(CRABMATE_FORCED_SKILL_NAME)
        );
        assert!(
            !text_of(&messages[0]).contains("ALPHA_BODY_ONLY"),
            "system 不得包含强制正文，否则该回合整段前缀失效"
        );
    }

    /// 续聊：注入紧跟真实 user；无注入时不追加任何条。
    #[test]
    fn existing_turn_appends_injection_right_after_user() {
        let mut messages = vec![
            Message::system_only("SYSTEM_BASE"),
            Message::user_only("旧问题"),
        ];
        append_turn_user_and_forced_skill(
            &mut messages,
            Message::user_only("新问题"),
            Some(Message::user_forced_skill_injection("ALPHA_BODY_ONLY")),
        );
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[2].name, None, "真实 user 不带 name");
        assert_eq!(
            messages[3].name.as_deref(),
            Some(CRABMATE_FORCED_SKILL_NAME)
        );
        assert!(!text_of(&messages[0]).contains("ALPHA_BODY_ONLY"));

        let mut plain = vec![Message::system_only("SYSTEM_BASE")];
        append_turn_user_and_forced_skill(&mut plain, Message::user_only("只读问题"), None);
        assert_eq!(plain.len(), 2, "无 /<id> 时不得追加注入条");
    }
}
