//! `POST /chat`、`/chat/stream`、`/chat/approval`、`/chat/branch`。

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use tokio::sync::mpsc;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use super::super::app_state::{AppState, ConversationTurnSeed};
use super::conflict::conversation_conflict_api_error;
use super::parse::{
    ensure_bearer_api_key_for_chat, normalize_agent_role, normalize_approval_session_id,
    normalize_chat_image_urls, normalize_client_conversation_id, parse_client_llm_override,
    parse_execution_mode_override, parse_executor_llm_override, parse_optional_chat_temperature,
    parse_seed_override_from_body,
};
use crate::agent_role_turn::maybe_apply_mid_session_agent_role_switch;
use crate::chat_job_queue;
use crate::clarification_questionnaire::{
    ClarifyAnswersNormalized, merge_user_text_with_clarification_answers,
    normalize_clarify_questionnaire_answers_raw,
};
use crate::context_bootstrap::conversation_turn_bootstrap::{
    compose_new_conversation_messages, first_turn_project_context_user_message_for_web,
};
use crate::conversation_store::SaveConversationOutcome;
use crate::memory::agent_memory::load_memory_snippet;
use crate::redact;
use crate::types::{
    CommandApprovalDecision, Message, filter_messages_for_web_client_snapshot,
    message_user_with_images,
};
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use crate::web::http_types::chat::{
    ApiError, ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBody,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBody, ChatResponseBody, ConversationMessagesQuery,
    ConversationMessagesResponseBody, StreamResumeBody,
};

fn sse_event_with_id(seq: u64, data: String) -> Result<Event, Infallible> {
    Ok(Event::default().id(seq.to_string()).data(data))
}

fn resolve_skills_base_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    if workspace_root.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        workspace_root.to_path_buf()
    }
}

fn resolve_skills_dir_path(
    base_dir: &std::path::Path,
    skills_dir: &str,
) -> Result<std::path::PathBuf, String> {
    let raw = skills_dir.trim();
    if raw.is_empty() {
        return Err("skills_dir 为空".to_string());
    }
    let p = std::path::Path::new(raw);
    Ok(if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    })
}

fn merge_system_prompt_with_workspace_skills_for_web(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
    skills_top_k: usize,
    workspace_root: &std::path::Path,
    user_text: &str,
) -> String {
    let base_dir = resolve_skills_base_dir(workspace_root);
    crate::config::skills::merge_system_prompt_with_skills_selected(
        system_prompt.clone(),
        skills_enabled,
        skills_dir,
        skills_max_chars,
        base_dir.as_path(),
        user_text,
        skills_top_k,
    )
    .unwrap_or(system_prompt)
}

fn classify_web_builtin_command(input: &str) -> Option<&'static str> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("/skills") {
        return Some("skills");
    }
    if s.eq_ignore_ascii_case("/skills list") {
        return Some("skills_list");
    }
    None
}

#[derive(Debug, Clone)]
struct SkillFileInfo {
    display_path: String,
    content: String,
    skill_name: Option<String>,
}

fn parse_skill_name_from_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("name:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn is_markdown_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn list_skill_files_for_web_builtin(
    skills_dir: &str,
    base_dir: &std::path::Path,
) -> Result<Vec<SkillFileInfo>, String> {
    let base = resolve_skills_dir_path(base_dir, skills_dir)?;
    if !base.exists() {
        return Ok(Vec::new());
    }
    if !base.is_dir() {
        return Err(format!("skills_dir 不是目录: {}", base.display()));
    }
    let mut out: Vec<SkillFileInfo> = Vec::new();
    for entry in std::fs::read_dir(&base).map_err(|e| format!("无法读取 skills_dir: {e}"))? {
        let Ok(entry) = entry else {
            continue;
        };
        let child = entry.path();
        let skill_path = if child.is_file() && is_markdown_file(&child) {
            child
        } else {
            continue;
        };
        if !skill_path.is_file() {
            continue;
        }
        let display = skill_path
            .strip_prefix(base_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| skill_path.display().to_string());
        let content = std::fs::read_to_string(&skill_path)
            .map_err(|e| format!("读取技能文件失败 {}: {e}", skill_path.display()))?;
        out.push(SkillFileInfo {
            display_path: display,
            skill_name: parse_skill_name_from_frontmatter(&content),
            content,
        });
    }
    out.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(out)
}

fn split_loaded_skills_by_budget(
    files: &[SkillFileInfo],
    max_chars: usize,
) -> (Vec<SkillFileInfo>, Vec<SkillFileInfo>) {
    // 与 `config::skills::render_skills_appendix` 的模板保持一致。
    let mut used = "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n"
        .chars()
        .count();
    let mut loaded: Vec<SkillFileInfo> = Vec::new();
    let mut skipped: Vec<SkillFileInfo> = Vec::new();
    for f in files {
        let per_file = format!(
            "\n\n---\n技能文件: {}\n\n{}",
            f.display_path,
            f.content.trim()
        );
        let need = per_file.chars().count();
        if used + need <= max_chars {
            used += need;
            loaded.push(f.clone());
        } else {
            skipped.push(f.clone());
        }
    }
    (loaded, skipped)
}

async fn run_web_builtin_command(state: &Arc<AppState>, command: &str) -> Option<String> {
    match classify_web_builtin_command(command)? {
        "skills" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills_max_chars;
            let dir = cfg.skills_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());

            let text = match list_skill_files_for_web_builtin(&dir, base_dir.as_path()) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：`{dir}`\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    format!(
                        "skills 概览：共 {} 个文件，按上限预计完整加载 {} 个，未完整加载 {} 个。\n目录：`{}`\n上限：skills_max_chars={}\n\n输入 `/skills list` 查看具体文件。",
                        files.len(),
                        loaded.len(),
                        skipped.len(),
                        dir,
                        max_chars
                    )
                }
                Err(e) => format!("读取 skills 失败：{e}"),
            };
            Some(text)
        }
        "skills_list" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills_max_chars;
            let dir = cfg.skills_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());
            let text = match list_skill_files_for_web_builtin(&dir, base_dir.as_path()) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：`{dir}`\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    let loaded_lines = if loaded.is_empty() {
                        "- （无）".to_string()
                    } else {
                        loaded
                            .iter()
                            .map(|f| {
                                let name = f.skill_name.as_deref().unwrap_or("未声明 name");
                                format!("- `{}` (name: `{}`)", f.display_path, name)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let skipped_lines = if skipped.is_empty() {
                        "- （无）".to_string()
                    } else {
                        skipped
                            .iter()
                            .map(|f| {
                                let name = f.skill_name.as_deref().unwrap_or("未声明 name");
                                format!("- `{}` (name: `{}`)", f.display_path, name)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    format!(
                        "当前已加载（完整进入 system）skills：\n{}\n\n未完整加载（受上限影响）skills：\n{}\n\n目录：`{}`\n上限：skills_max_chars={}（扫描总数：{}）",
                        loaded_lines,
                        skipped_lines,
                        dir,
                        max_chars,
                        files.len()
                    )
                }
                Err(e) => format!("读取 skills 失败：{e}"),
            };
            Some(text)
        }
        _ => None,
    }
}

fn reject_if_client_sse_protocol_invalid(
    client_sse_protocol: Option<u8>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(v) = client_sse_protocol else {
        return Ok(());
    };
    if v == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_SSE_CLIENT_PROTOCOL",
                message: "client_sse_protocol 非法（须为 1～255）".to_string(),
                reason_code: None,
            }),
        ));
    }
    if v > crate::sse::protocol::SSE_PROTOCOL_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "SSE_CLIENT_TOO_NEW",
                message: "客户端声明的 SSE 协议版本高于服务端，请升级服务器或更换匹配的前端构建"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    Ok(())
}

fn parse_last_event_id(headers: &HeaderMap) -> Option<u64> {
    let raw = headers.get(axum::http::HeaderName::from_static("last-event-id"))?;
    let s = raw.to_str().ok()?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

/// `chat_stream_handler` 前半段：校验并解析请求体（降低主 handler 圈复杂度）。
struct ChatStreamRequestParsed {
    resume: Option<StreamResumeBody>,
    image_urls: Vec<String>,
    clarify: Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
    user_trim: String,
    conversation_id: String,
    agent_role: Option<String>,
    temperature_override: Option<f32>,
    seed_override: crate::LlmSeedOverride,
    llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    executor_llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    execution_mode_override: Option<chat_job_queue::WebExecutionModeOverride>,
}

fn parse_chat_stream_request(
    state: &Arc<AppState>,
    body: &ChatRequestBody,
) -> Result<ChatStreamRequestParsed, (StatusCode, Json<ApiError>)> {
    let resume = body.stream_resume.clone();
    let image_urls = normalize_chat_image_urls(&body.image_urls).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_IMAGE_URLS",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let clarify = if let Some(ref c) = body.clarify_questionnaire_answers {
        normalize_clarify_questionnaire_answers_raw(c.questionnaire_id.clone(), c.answers.clone())
            .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
    } else {
        None
    };
    let user_trim = body.message.trim().to_string();
    if user_trim.is_empty() && resume.is_none() && image_urls.is_empty() && clarify.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空（若仅发图须至少附带一张图片；澄清问卷作答可单独提交）"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let temperature_override = parse_optional_chat_temperature(body.temperature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_TEMPERATURE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let seed_override = parse_seed_override_from_body(body.seed, body.seed_policy.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_SEED",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let llm_override = parse_client_llm_override(body.client_llm.clone()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CLIENT_LLM",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let executor_llm_override =
        parse_executor_llm_override(body.executor_llm.clone()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTOR_LLM",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let execution_mode_override = parse_execution_mode_override(body.execution_mode.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTION_MODE",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    Ok(ChatStreamRequestParsed {
        resume,
        image_urls,
        clarify,
        user_trim,
        conversation_id,
        agent_role,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
    })
}

async fn build_messages_for_turn(
    state: &Arc<AppState>,
    conversation_id: &str,
    user_msg: &str,
    image_urls: &[String],
    agent_role: Option<&str>,
) -> Result<ConversationTurnSeed, String> {
    let root_str = state.effective_workspace_path().await;
    let workspace_is_set = state.workspace_is_set().await;
    let root = std::path::PathBuf::from(root_str);
    let last_user = if image_urls.is_empty() {
        Message::user_only(user_msg.to_string())
    } else {
        message_user_with_images(user_msg, image_urls)
    };
    if let Some(mut seed) = state.load_conversation_seed(conversation_id).await {
        let persisted = seed.persisted_active_agent_role.clone();
        {
            let cfg = state.cfg.read().await;
            if let Some(id) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
                cfg.system_prompt_for_new_conversation(Some(id))
                    .map_err(|e| e.to_string())?;
            }
            maybe_apply_mid_session_agent_role_switch(
                &cfg,
                &mut seed.messages,
                persisted.as_deref(),
                agent_role,
            )?;
            let role_for_turn = agent_role
                .and_then(|s| {
                    let t = s.trim();
                    if t.is_empty() { None } else { Some(t) }
                })
                .or(persisted.as_deref());
            let system_for_turn = cfg
                .system_prompt_for_new_conversation(role_for_turn)
                .map_err(|e| e.to_string())?
                .to_string();
            let system_for_turn = crate::tool_stats::augment_system_prompt(&system_for_turn, &cfg);
            let system_for_turn = if workspace_is_set {
                merge_system_prompt_with_workspace_skills_for_web(
                    system_for_turn,
                    cfg.skills_enabled,
                    cfg.skills_dir.as_str(),
                    cfg.skills_max_chars,
                    cfg.skills_top_k,
                    root.as_path(),
                    user_msg,
                )
            } else {
                system_for_turn
            };
            if let Some(first) = seed.messages.first_mut()
                && first.role == "system"
            {
                first.content = Some(crate::types::MessageContent::Text(system_for_turn));
            }
        }
        seed.messages.push(last_user);
        return Ok(seed);
    }
    let cfg = state.cfg.read().await;
    let system_for_turn = cfg
        .system_prompt_for_new_conversation(agent_role)
        .map_err(|e| e.to_string())?
        .to_string();
    let system_for_turn = crate::tool_stats::augment_system_prompt(&system_for_turn, &cfg);
    let system_for_turn = if workspace_is_set {
        merge_system_prompt_with_workspace_skills_for_web(
            system_for_turn,
            cfg.skills_enabled,
            cfg.skills_dir.as_str(),
            cfg.skills_max_chars,
            cfg.skills_top_k,
            root.as_path(),
            user_msg,
        )
    } else {
        system_for_turn
    };
    let memory_snippet = if workspace_is_set && cfg.agent_memory_file_enabled {
        load_memory_snippet(
            &root,
            cfg.agent_memory_file.as_str(),
            cfg.agent_memory_file_max_chars,
        )
    } else {
        None
    };

    let combined = first_turn_project_context_user_message_for_web(
        workspace_is_set,
        root.as_path(),
        &cfg,
        memory_snippet,
    )
    .await;
    let messages = compose_new_conversation_messages(&system_for_turn, combined, Some(last_user));
    Ok(ConversationTurnSeed {
        messages,
        expected_revision: None,
        persisted_active_agent_role: None,
    })
}

/// 与 `chat_handler` 共用的 JSON 入队：解析 `@`、组装首轮消息、选定工作目录。
pub(crate) struct PreparedJsonChatEnqueue {
    pub(crate) conversation_id: String,
    pub(crate) turn_seed: ConversationTurnSeed,
    pub(crate) work_dir: PathBuf,
    pub(crate) workspace_is_set: bool,
    pub(crate) msg_for_log: String,
}

pub(crate) async fn prepare_json_chat_enqueue(
    state: &Arc<AppState>,
    user_trim: &str,
    clarify: Option<ClarifyAnswersNormalized>,
    image_urls: &[String],
    agent_role: Option<String>,
    conversation_id: String,
) -> Result<PreparedJsonChatEnqueue, (StatusCode, Json<ApiError>)> {
    let eff_ws_raw = state.effective_workspace_path().await;
    let eff_ws = eff_ws_raw.trim().to_string();
    if eff_ws.is_empty() && user_trim.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "WORKSPACE_NOT_SET",
                message: "未设置工作区：无法在消息中使用 `@` 引用工作区内文件。请先在侧栏工作区面板选择或提交目录。"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    let work_dir_for_expand = std::path::PathBuf::from(eff_ws_raw.clone());
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(user_trim, work_dir_for_expand.as_path(), &cfg)
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "INVALID_AT_FILE_REF",
                        message: e,
                        reason_code: None,
                    }),
                )
            })?
    };
    let msg = merge_user_text_with_clarification_answers(msg, clarify);
    let turn_seed = build_messages_for_turn(
        state,
        &conversation_id,
        &msg,
        image_urls,
        agent_role.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let workspace_is_set = state.workspace_is_set().await;
    let work_dir_for_job = if eff_ws.is_empty() {
        let cfg = state.cfg.read().await;
        std::path::PathBuf::from(cfg.run_command_working_dir.clone())
    } else {
        std::path::PathBuf::from(eff_ws.clone())
    };
    Ok(PreparedJsonChatEnqueue {
        conversation_id,
        turn_seed,
        work_dir: work_dir_for_job,
        workspace_is_set,
        msg_for_log: msg,
    })
}

/// `POST /chat` / **`POST /chat/async`** 共用：校验请求体（**不含** `prepare_json_chat_enqueue` 与内置命令）。
struct ParsedChatRequestForEnqueue {
    image_urls: Vec<String>,
    clarify: Option<ClarifyAnswersNormalized>,
    user_trim: String,
    conversation_id: String,
    agent_role: Option<String>,
    temperature_override: Option<f32>,
    seed_override: crate::types::LlmSeedOverride,
    llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    executor_llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    execution_mode_override: Option<chat_job_queue::WebExecutionModeOverride>,
}

async fn parse_chat_request_for_enqueue(
    state: &Arc<AppState>,
    body: &ChatRequestBody,
) -> Result<ParsedChatRequestForEnqueue, (StatusCode, Json<ApiError>)> {
    let image_urls = normalize_chat_image_urls(&body.image_urls).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_IMAGE_URLS",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let clarify = if let Some(ref c) = body.clarify_questionnaire_answers {
        normalize_clarify_questionnaire_answers_raw(c.questionnaire_id.clone(), c.answers.clone())
            .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
    } else {
        None
    };
    let user_trim = body.message.trim();
    if user_trim.is_empty() && image_urls.is_empty() && clarify.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空（若仅发图须至少附带一张图片；澄清问卷作答可单独提交）"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let temperature_override = parse_optional_chat_temperature(body.temperature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_TEMPERATURE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let seed_override = parse_seed_override_from_body(body.seed, body.seed_policy.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_SEED",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let llm_override = parse_client_llm_override(body.client_llm.clone()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CLIENT_LLM",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let executor_llm_override =
        parse_executor_llm_override(body.executor_llm.clone()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTOR_LLM",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let execution_mode_override = parse_execution_mode_override(body.execution_mode.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTION_MODE",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    ensure_bearer_api_key_for_chat(state, &llm_override).await?;
    Ok(ParsedChatRequestForEnqueue {
        image_urls,
        clarify,
        user_trim: user_trim.to_string(),
        conversation_id,
        agent_role,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
    })
}

async fn enqueue_and_wait_json_chat(
    state: Arc<AppState>,
    parsed: ParsedChatRequestForEnqueue,
) -> Result<(Vec<Message>, u64), (StatusCode, Json<ApiError>)> {
    let PreparedJsonChatEnqueue {
        conversation_id,
        turn_seed,
        work_dir: work_dir_for_job,
        workspace_is_set,
        msg_for_log: msg,
    } = prepare_json_chat_enqueue(
        &state,
        parsed.user_trim.as_str(),
        parsed.clarify,
        &parsed.image_urls,
        parsed.agent_role.clone(),
        parsed.conversation_id.clone(),
    )
    .await?;
    let job_id = state.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    debug!(
        target: "crabmate",
        "chat json 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(&msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat json 任务入队 job_id={}", job_id);
    state
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            job_id,
            queue_deps: state.chat_queue_job_deps.clone(),
            app: state.clone(),
            conversation_id: conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: parsed.agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir: work_dir_for_job,
            workspace_is_set,
            temperature_override: parsed.temperature_override,
            seed_override: parsed.seed_override,
            llm_override: parsed.llm_override.clone(),
            executor_llm_override: parsed.executor_llm_override.clone(),
            execution_mode_override: parsed.execution_mode_override,
            reply_tx,
        })
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiError {
                    code: "QUEUE_FULL",
                    message: format!(
                        "对话任务队列已满（最多等待 {} 个），请稍后重试",
                        e.max_pending
                    ),
                    reason_code: None,
                }),
            )
        })?;
    let messages = reply_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "INTERNAL_ERROR",
                    message: "对话任务被取消或内部错误".to_string(),
                    reason_code: None,
                }),
            )
        })?
        .map_err(|e| match e {
            chat_job_queue::ChatJsonJobFailure::ConversationConflict => {
                conversation_conflict_api_error()
            }
            chat_job_queue::ChatJsonJobFailure::Agent(err) => {
                error!(
                    target: "crabmate",
                    "chat json 队列任务失败 job_id={} err_kind=agent_turn {}",
                    job_id,
                    err.diag_log_kv(),
                );
                let status = err.suggested_http_status();
                let body = err.http_api_error();
                (status, Json(body))
            }
        })?;
    Ok((messages, job_id))
}

pub(crate) async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Json<ChatResponseBody>, (StatusCode, Json<ApiError>)> {
    let parsed = parse_chat_request_for_enqueue(&state, &body).await?;
    if let Some(reply) = run_web_builtin_command(&state, parsed.user_trim.as_str()).await {
        return Ok(Json(ChatResponseBody {
            reply,
            conversation_id: parsed.conversation_id,
            conversation_revision: None,
        }));
    }
    let cid = parsed.conversation_id.clone();
    let (messages, _) = enqueue_and_wait_json_chat(state.clone(), parsed).await?;
    let reply = messages
        .last()
        .and_then(|m| crate::types::message_content_as_str(&m.content))
        .unwrap_or("")
        .to_string();
    let conversation_revision = state
        .load_conversation_seed(&cid)
        .await
        .and_then(|s| s.expected_revision);
    Ok(Json(ChatResponseBody {
        reply,
        conversation_id: cid,
        conversation_revision,
    }))
}

fn normalize_optional_webhook_url(
    raw: Option<String>,
) -> Result<Option<reqwest::Url>, (StatusCode, Json<ApiError>)> {
    let Some(s) = raw else {
        return Ok(None);
    };
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let u = reqwest::Url::parse(t).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("INVALID_WEBHOOK_URL", format!("{e}"))),
        )
    })?;
    if matches!(u.scheme(), "http" | "https") {
        Ok(Some(u))
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_WEBHOOK_URL",
                "webhook_url 仅支持 http 或 https",
            )),
        ))
    }
}

fn normalize_webhook_secret(
    raw: Option<String>,
) -> Result<Option<String>, (StatusCode, Json<ApiError>)> {
    let Some(s) = raw else {
        return Ok(None);
    };
    let t = s.trim().to_string();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() > 256 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_WEBHOOK_SECRET",
                "webhook_secret 过长（最多 256 字符）",
            )),
        ));
    }
    Ok(Some(t))
}

async fn post_chat_job_webhook(
    client: &reqwest::Client,
    url: &reqwest::Url,
    secret: Option<&str>,
    payload: &crate::web::async_chat_job::WebhookPayload<'_>,
) {
    let mut req = client.post(url.clone()).json(payload);
    if let Some(s) = secret {
        if let Ok(v) = HeaderValue::from_str(s) {
            req = req.header("X-Crabmate-Webhook-Secret", v);
        } else {
            warn!(target: "crabmate", "webhook_secret 含非法 HTTP 头字符，跳过 X-Crabmate-Webhook-Secret");
        }
    }
    match req.timeout(std::time::Duration::from_secs(30)).send().await {
        Ok(resp) if resp.status().is_success() => {
            debug!(target: "crabmate", "async chat webhook ok status={}", resp.status());
        }
        Ok(resp) => {
            warn!(
                target: "crabmate",
                "async chat webhook non-success status={}",
                resp.status()
            );
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "async chat webhook request failed: {}",
                e
            );
        }
    }
}

async fn run_async_chat_json_job(
    state: Arc<AppState>,
    job_id: u64,
    reply_rx: tokio::sync::oneshot::Receiver<
        Result<Vec<Message>, chat_job_queue::ChatJsonJobFailure>,
    >,
    conversation_id: String,
    webhook_url: Option<reqwest::Url>,
    webhook_secret: Option<String>,
) {
    {
        let mut g = state.async_chat_jobs.write().await;
        if let Some(r) = g.get_mut(&job_id) {
            r.status = crate::web::async_chat_job::ChatAsyncJobStatus::Running;
        }
    }

    let messages_res = reply_rx.await.ok();

    let (status_str, reply, revision, err_api) = match messages_res {
        Some(Ok(messages)) => {
            let reply = messages
                .last()
                .and_then(|m| crate::types::message_content_as_str(&m.content))
                .unwrap_or("")
                .to_string();
            let revision = state
                .load_conversation_seed(&conversation_id)
                .await
                .and_then(|s| s.expected_revision);
            ("completed", Some(reply), revision, None)
        }
        Some(Err(chat_job_queue::ChatJsonJobFailure::ConversationConflict)) => {
            let e = ApiError {
                code: super::conflict::CONVERSATION_CONFLICT_CODE,
                message: super::conflict::CONVERSATION_CONFLICT_MESSAGE.to_string(),
                reason_code: None,
            };
            ("failed", None, None, Some(e))
        }
        Some(Err(chat_job_queue::ChatJsonJobFailure::Agent(err))) => {
            error!(
                target: "crabmate",
                "chat async job failed job_id={} err_kind=agent_turn {}",
                job_id,
                err.diag_log_kv(),
            );
            let body = err.http_api_error();
            ("failed", None, None, Some(body))
        }
        None => (
            "failed",
            None,
            None,
            Some(ApiError::new("INTERNAL_ERROR", "对话任务被取消或内部错误")),
        ),
    };

    {
        let mut g = state.async_chat_jobs.write().await;
        if let Some(r) = g.get_mut(&job_id) {
            r.status = if status_str == "completed" {
                crate::web::async_chat_job::ChatAsyncJobStatus::Completed
            } else {
                crate::web::async_chat_job::ChatAsyncJobStatus::Failed
            };
            r.reply.clone_from(&reply);
            r.conversation_revision = revision;
            r.error = err_api.clone();
        }
    }

    if let Some(ref url) = webhook_url {
        let payload = crate::web::async_chat_job::WebhookPayload {
            job_id,
            status: status_str,
            conversation_id: conversation_id.as_str(),
            conversation_revision: revision,
            reply: reply.as_deref(),
            error: err_api.as_ref(),
        };
        post_chat_job_webhook(&state.client, url, webhook_secret.as_deref(), &payload).await;
    }
}

pub(crate) async fn chat_async_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatAsyncRequestBody>,
) -> Result<Json<ChatAsyncSubmitResponseBody>, (StatusCode, Json<ApiError>)> {
    if body.chat.stream_resume.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "ASYNC_STREAM_RESUME_UNSUPPORTED",
                "异步任务不支持 stream_resume；请使用 POST /chat/stream",
            )),
        ));
    }
    let parsed = parse_chat_request_for_enqueue(&state, &body.chat).await?;
    if run_web_builtin_command(&state, parsed.user_trim.as_str())
        .await
        .is_some()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "ASYNC_BUILTIN_UNSUPPORTED",
                "内置命令（如 /skills）请使用同步 POST /chat",
            )),
        ));
    }

    let webhook_url = normalize_optional_webhook_url(body.webhook_url)?;
    let webhook_secret = normalize_webhook_secret(body.webhook_secret)?;
    let conversation_id = parsed.conversation_id.clone();

    let job_id = state.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    {
        let mut g = state.async_chat_jobs.write().await;
        g.insert(
            job_id,
            crate::web::async_chat_job::ChatAsyncJobRecord {
                status: crate::web::async_chat_job::ChatAsyncJobStatus::Pending,
                conversation_id: conversation_id.clone(),
                created_at: std::time::Instant::now(),
                webhook_url: webhook_url.as_ref().map(|u| u.to_string()),
                webhook_secret: webhook_secret.clone(),
                reply: None,
                conversation_revision: None,
                error: None,
            },
        );
    }

    let PreparedJsonChatEnqueue {
        conversation_id: cid_enqueue,
        turn_seed,
        work_dir: work_dir_for_job,
        workspace_is_set,
        msg_for_log: msg,
    } = prepare_json_chat_enqueue(
        &state,
        parsed.user_trim.as_str(),
        parsed.clarify,
        &parsed.image_urls,
        parsed.agent_role.clone(),
        conversation_id.clone(),
    )
    .await?;

    let submit = state
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            job_id,
            queue_deps: state.chat_queue_job_deps.clone(),
            app: state.clone(),
            conversation_id: cid_enqueue,
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: parsed.agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir: work_dir_for_job,
            workspace_is_set,
            temperature_override: parsed.temperature_override,
            seed_override: parsed.seed_override,
            llm_override: parsed.llm_override.clone(),
            executor_llm_override: parsed.executor_llm_override.clone(),
            execution_mode_override: parsed.execution_mode_override,
            reply_tx,
        });

    if let Err(e) = submit {
        let mut g = state.async_chat_jobs.write().await;
        g.remove(&job_id);
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                code: "QUEUE_FULL",
                message: format!(
                    "对话任务队列已满（最多等待 {} 个），请稍后重试",
                    e.max_pending
                ),
                reason_code: None,
            }),
        ));
    }

    debug!(
        target: "crabmate",
        "chat async 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(&msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat async 任务入队 job_id={}", job_id);

    let st = state.clone();
    let cid = conversation_id.clone();
    let wurl = webhook_url.clone();
    let wsec = webhook_secret.clone();
    tokio::spawn(async move {
        run_async_chat_json_job(st, job_id, reply_rx, cid, wurl, wsec).await;
    });

    Ok(Json(ChatAsyncSubmitResponseBody {
        job_id,
        status: "pending",
        conversation_id,
    }))
}

pub(crate) async fn chat_job_status_handler(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<u64>,
) -> Result<Json<ChatJobStatusResponseBody>, (StatusCode, Json<ApiError>)> {
    let g = state.async_chat_jobs.read().await;
    let Some(rec) = g.get(&job_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "UNKNOWN_JOB",
                "不存在或未通过 POST /chat/async 创建的任务",
            )),
        ));
    };
    let status = match rec.status {
        crate::web::async_chat_job::ChatAsyncJobStatus::Pending => "pending",
        crate::web::async_chat_job::ChatAsyncJobStatus::Running => "running",
        crate::web::async_chat_job::ChatAsyncJobStatus::Completed => "completed",
        crate::web::async_chat_job::ChatAsyncJobStatus::Failed => "failed",
    };
    Ok(Json(ChatJobStatusResponseBody {
        job_id,
        status: status.to_string(),
        conversation_id: rec.conversation_id.clone(),
        reply: rec.reply.clone(),
        conversation_revision: rec.conversation_revision,
        error: rec.error.clone(),
    }))
}

pub(crate) async fn chat_approval_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatApprovalRequestBody>,
) -> Result<Json<ChatApprovalResponseBody>, (StatusCode, Json<ApiError>)> {
    let session_id = normalize_approval_session_id(&body.approval_session_id).ok_or((
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            code: "INVALID_APPROVAL_SESSION_ID",
            message: "approval_session_id 非法或为空".to_string(),
            reason_code: None,
        }),
    ))?;
    let decision = match body.decision.trim().to_ascii_lowercase().as_str() {
        "deny" => CommandApprovalDecision::Deny,
        "allow_once" => CommandApprovalDecision::AllowOnce,
        "allow_always" => CommandApprovalDecision::AllowAlways,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_APPROVAL_DECISION",
                    message: "decision 仅支持 deny / allow_once / allow_always".to_string(),
                    reason_code: None,
                }),
            ));
        }
    };
    let tx = {
        let guard = state.approval_sessions.read().await;
        guard.get(&session_id).cloned()
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ApiError {
            code: "APPROVAL_SESSION_NOT_FOUND",
            message: "审批会话不存在或已结束".to_string(),
            reason_code: None,
        }),
    ))?;
    if tx.send(decision).await.is_err() {
        debug!(
            target: "crabmate::sse_mpsc",
            "approval decision mpsc send failed: session_id={} receiver dropped",
            session_id
        );
        state.approval_sessions.write().await.remove(&session_id);
        return Err((
            StatusCode::GONE,
            Json(ApiError {
                code: "APPROVAL_SESSION_CLOSED",
                message: "审批会话已关闭".to_string(),
                reason_code: None,
            }),
        ));
    }
    Ok(Json(ChatApprovalResponseBody { ok: true }))
}

/// 将会话历史截断到前 N 条消息（`keep_message_count`），**同一** `conversation_id` 下继续对话。
pub(crate) async fn chat_branch_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatBranchRequestBody>,
) -> Result<Json<ChatBranchResponseBody>, (StatusCode, Json<ApiError>)> {
    let conversation_id =
        normalize_client_conversation_id(Some(&body.conversation_id)).map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: msg,
                    reason_code: None,
                }),
            )
        })?;
    let Some(cid) = conversation_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CONVERSATION_ID",
                message: "conversation_id 不能为空".to_string(),
                reason_code: None,
            }),
        ));
    };
    let ord = usize::try_from(body.before_user_ordinal).unwrap_or(usize::MAX);
    let seed = state.load_conversation_seed(&cid).await;
    let Some(seed) = seed else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let Some(exp) = seed.expected_revision else {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_REVISION_UNKNOWN",
                message: "无法分支：缺少 revision 信息".to_string(),
                reason_code: None,
            }),
        ));
    };
    if exp != body.expected_revision {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_CONFLICT",
                message: "revision 不匹配，请刷新后重试".to_string(),
                reason_code: None,
            }),
        ));
    }
    match state
        .truncate_conversation_before_user_ordinal_if_revision(
            cid.clone(),
            ord,
            body.expected_revision,
        )
        .await
    {
        SaveConversationOutcome::Saved => {}
        SaveConversationOutcome::Conflict => {
            return Err((
                StatusCode::CONFLICT,
                Json(ApiError {
                    code: "CONVERSATION_CONFLICT",
                    message: "会话已被其他请求更新或 revision 不匹配".to_string(),
                    reason_code: None,
                }),
            ));
        }
    }
    let new_rev = state
        .load_conversation_seed(&cid)
        .await
        .and_then(|s| s.expected_revision)
        .unwrap_or(body.expected_revision);
    Ok(Json(ChatBranchResponseBody {
        ok: true,
        revision: new_rev,
    }))
}

/// 只读拉取服务端已持久化的会话消息与 revision（Web 刷新后与 `conversation_id` 对齐）。
pub(crate) async fn conversation_messages_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConversationMessagesQuery>,
) -> Result<Json<ConversationMessagesResponseBody>, (StatusCode, Json<ApiError>)> {
    let conversation_id =
        normalize_client_conversation_id(Some(&q.conversation_id)).map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: msg,
                    reason_code: None,
                }),
            )
        })?;
    let Some(cid) = conversation_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CONVERSATION_ID",
                message: "conversation_id 不能为空".to_string(),
                reason_code: None,
            }),
        ));
    };
    let Some(seed) = state.load_conversation_seed(&cid).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let Some(revision) = seed.expected_revision else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let messages = filter_messages_for_web_client_snapshot(&seed.messages);
    let active_agent_role = seed
        .persisted_active_agent_role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Json(ConversationMessagesResponseBody {
        conversation_id: cid,
        revision,
        active_agent_role,
        messages,
    }))
}

/// 流式 chat：返回 SSE，每个 event 的 **`id`** 为单调序号（断线重连与 **`Last-Event-ID`** / **`stream_resume`**），`data` 为控制面 JSON 或正文 delta。
pub(crate) async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ChatRequestBody>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let p = parse_chat_stream_request(&state, &body)?;
    let resume = p.resume.as_ref();
    ensure_bearer_api_key_for_chat(&state, &p.llm_override).await?;
    if let Some(reply) = run_web_builtin_command(&state, p.user_trim.as_str()).await {
        let stream =
            stream::iter(vec![(1_u64, reply)]).map(|(seq, data)| sse_event_with_id(seq, data));
        let mut resp = Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response();
        if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
            resp.headers_mut().insert("x-conversation-id", v);
        }
        return Ok(resp);
    }

    if let Some(sr) = resume {
        let job_id = sr.job_id;
        if !state.sse_stream_hub.has_job(job_id) {
            return Err((
                StatusCode::GONE,
                Json(ApiError {
                    code: "STREAM_JOB_GONE",
                    message: "流式任务已结束或不在本进程内存中，无法重连".to_string(),
                    reason_code: None,
                }),
            ));
        }
        let after_header = parse_last_event_id(&headers).unwrap_or(0);
        let after_body = sr.after_seq.unwrap_or(0);
        let after_seq = after_header.max(after_body);
        let Some(sub) = state.sse_stream_hub.subscribe(job_id) else {
            return Err((
                StatusCode::GONE,
                Json(ApiError {
                    code: "STREAM_JOB_GONE",
                    message: "流式任务已结束或不在本进程内存中，无法重连".to_string(),
                    reason_code: None,
                }),
            ));
        };
        let replay = state
            .sse_stream_hub
            .replay_after(job_id, after_seq)
            .unwrap_or_default();
        let max_replayed = replay.last().map(|(s, _)| *s).unwrap_or(after_seq);
        info!(
            target: "crabmate",
            "chat stream 断线重连 job_id={} after_seq={} replayed={}",
            job_id,
            after_seq,
            replay.len()
        );
        let replay_st = stream::iter(replay).map(|(seq, data)| sse_event_with_id(seq, data));
        let live_st = BroadcastStream::new(sub).filter_map(move |item| {
            std::future::ready(match item {
                Ok((seq, data)) if seq > max_replayed => Some(sse_event_with_id(seq, data)),
                Ok(_) => None,
                Err(BroadcastStreamRecvError::Lagged(n)) => {
                    warn!(
                        target: "crabmate",
                        "chat stream 重连 broadcast lag job_id={} skipped={}",
                        job_id,
                        n
                    );
                    None
                }
            })
        });
        let merged = replay_st.chain(live_st);
        let mut resp = Sse::new(merged)
            .keep_alive(KeepAlive::default())
            .into_response();
        if let Ok(v) = HeaderValue::from_str(&job_id.to_string()) {
            resp.headers_mut().insert("x-stream-job-id", v);
        }
        if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
            resp.headers_mut().insert("x-conversation-id", v);
        }
        return Ok(resp);
    }

    let eff_ws_raw = state.effective_workspace_path().await;
    let eff_ws = eff_ws_raw.trim().to_string();
    if eff_ws.is_empty() && p.user_trim.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "WORKSPACE_NOT_SET",
                message: "未设置工作区：无法在消息中使用 `@` 引用工作区内文件。请先在侧栏工作区面板选择或提交目录。"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    let work_dir_for_expand = std::path::PathBuf::from(eff_ws_raw.clone());
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(&p.user_trim, work_dir_for_expand.as_path(), &cfg)
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "INVALID_AT_FILE_REF",
                        message: e,
                        reason_code: None,
                    }),
                )
            })?
    };
    let msg = merge_user_text_with_clarification_answers(msg, p.clarify);
    let turn_seed = build_messages_for_turn(
        &state,
        &p.conversation_id,
        &msg,
        &p.image_urls,
        p.agent_role.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let workspace_is_set = state.workspace_is_set().await;
    let work_dir_for_job = if eff_ws.is_empty() {
        let cfg = state.cfg.read().await;
        std::path::PathBuf::from(cfg.run_command_working_dir.clone())
    } else {
        std::path::PathBuf::from(eff_ws.clone())
    };
    let approval_session_id = match body.approval_session_id.as_deref() {
        Some(v) => Some(normalize_approval_session_id(v).ok_or((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_APPROVAL_SESSION_ID",
                message: "approval_session_id 非法或为空".to_string(),
                reason_code: None,
            }),
        ))?),
        None => None,
    };
    let mut web_approval_session = None;
    if let Some(session_id) = approval_session_id.as_ref() {
        let (approval_tx, approval_rx) = mpsc::channel::<CommandApprovalDecision>(8);
        state
            .approval_sessions
            .write()
            .await
            .insert(session_id.clone(), approval_tx);
        web_approval_session = Some(chat_job_queue::WebApprovalSession {
            session_id: session_id.clone(),
            approval_rx,
        });
    }
    let job_id = state.chat_queue.next_job_id();
    let (tx, rx) = mpsc::channel::<(u64, String)>(1024);
    debug!(
        target: "crabmate",
        "chat stream 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(&msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat stream 任务入队 job_id={}", job_id);
    if let Err(e) = state
        .chat_queue
        .try_submit_stream(chat_job_queue::StreamSubmitParams {
            job_id,
            queue_deps: state.chat_queue_job_deps.clone(),
            app: state.clone(),
            conversation_id: p.conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: p.agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir: work_dir_for_job,
            workspace_is_set,
            temperature_override: p.temperature_override,
            seed_override: p.seed_override,
            llm_override: p.llm_override,
            executor_llm_override: p.executor_llm_override,
            execution_mode_override: p.execution_mode_override,
            stream_event_tx: tx,
            web_approval_session,
        })
    {
        if let Some(session_id) = approval_session_id {
            state.approval_sessions.write().await.remove(&session_id);
        }
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                code: "QUEUE_FULL",
                message: format!(
                    "对话任务队列已满（最多等待 {} 个），请稍后重试",
                    e.max_pending
                ),
                reason_code: None,
            }),
        ));
    }
    let stream = ReceiverStream::new(rx).map(|(seq, data)| sse_event_with_id(seq, data));
    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
        resp.headers_mut().insert("x-conversation-id", v);
    }
    if let Ok(v) = HeaderValue::from_str(&job_id.to_string()) {
        resp.headers_mut().insert("x-stream-job-id", v);
    }
    Ok(resp)
}
