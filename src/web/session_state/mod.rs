//! `POST /conversation/state`：会话级运行态意图（`agent_role` / `session_mode` /
//! `permission_level`）的**空闲态写入口**（设计 `docs/design/permission_levels.md` §16.1）。
//!
//! 为什么需要它：这三件套原先只在**回合结束时**落盘（`chat_job_queue::stream_finish`），
//! 所以 Client 在空闲态切换开关时服务端完全不知情——刷新页面或换个标签页又回到旧档。
//! 本端点把「写意图」从回合里拆出来，并且**不碰** `messages_json` / `revision`
//! （见 [`crate::conversation_store::set_active_state`]），因此不会让进行中回合的落盘
//! 变成 CAS `Conflict`。
//!
//! 生效仍是**回合粒度**：`PermissionCaps` 在 `prepare_turn_cfg_and_permission` 里一次算好，
//! 中途切档对当前这一回合无效，从下一回合开始。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::cm_api_contract::ApiError;
use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::session_state::{ConversationStatePatch, ConversationStateView};
use crate::cm_types::parse_optional_permission_level;
use crate::web::app_state::ConversationTurnSeed;
use crate::web::app_state_facets::WebChatAppFacet;
use crate::web::chat_handlers::normalize_agent_role;
use crate::web::permission::{PermissionViewInput, build_permission_view};

type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;

fn bad_request(code: &'static str, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (StatusCode::BAD_REQUEST, Json(ApiError::new(code, message)))
}

fn not_found(code: &'static str, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (StatusCode::NOT_FOUND, Json(ApiError::new(code, message)))
}

/// 校验并规范化后的请求（三个字段都表示「要写入的意图」，已过 trim + 枚举校验）。
struct ParsedPatch {
    conversation_id: String,
    permission_level: Option<String>,
    session_mode: Option<String>,
    agent_role: Option<String>,
}

impl ParsedPatch {
    /// 未知取值一律 400：静默回落会把客户端的显式选择当成没发生。
    fn parse(body: &ConversationStatePatch) -> ApiResult<Self> {
        let conversation_id =
            crate::web::normalize_client_conversation_id(body.conversation_id.as_deref())
                .map_err(|msg| bad_request(error_codes::INVALID_CONVERSATION_ID, msg))?
                .ok_or_else(|| {
                    bad_request(
                        error_codes::INVALID_CONVERSATION_ID,
                        "conversation_id 不能为空",
                    )
                })?;
        let permission_level = parse_optional_permission_level(body.permission_level.as_deref())
            .map_err(|e| bad_request(error_codes::INVALID_PERMISSION_LEVEL, e))?
            .map(|level| level.as_str().to_string());
        let session_mode =
            crate::cm_types::parse_optional_session_mode(body.session_mode.as_deref())
                .map_err(|e| bad_request(error_codes::INVALID_SESSION_MODE, e))?
                .map(|mode| mode.as_str().to_string());
        let agent_role = normalize_agent_role(body.agent_role.as_deref())
            .map_err(|msg| bad_request(error_codes::INVALID_AGENT_ROLE, msg))?;
        if permission_level.is_none() && session_mode.is_none() && agent_role.is_none() {
            return Err(bad_request(
                error_codes::EMPTY_CONVERSATION_STATE_PATCH,
                "至少提供一个字段：permission_level / session_mode / agent_role",
            ));
        }
        Ok(Self {
            conversation_id,
            permission_level,
            session_mode,
            agent_role,
        })
    }
}

/// 会话不存在 → 404（**不做** upsert：空 `messages_json` 行会污染会话列表与 TTL 淘汰）。
async fn load_seed(state: &WebChatAppFacet, conversation_id: &str) -> ApiResult<ConversationTurnSeed> {
    state
        .load_conversation_seed(conversation_id)
        .await
        .ok_or_else(|| {
            not_found(
                error_codes::CONVERSATION_NOT_FOUND,
                "会话不存在或已过期",
            )
        })
}

/// `POST /conversation/state`：写入会话级意图并回带更新后的权限投影。
pub(crate) async fn conversation_state_handler(
    State(state): State<WebChatAppFacet>,
    Json(body): Json<ConversationStatePatch>,
) -> ApiResult<Json<ConversationStateView>> {
    let patch = ParsedPatch::parse(&body)?;
    if let Some(role) = patch.agent_role.as_deref() {
        // 与 `GET /permission` 同口径：未知角色必须报错，否则下一回合工具策略会按一个不存在的角色解析。
        let cfg = state.cfg.read().await;
        if !cfg.roles_prompts.agent_roles.contains_key(role) {
            return Err(bad_request(
                error_codes::INVALID_AGENT_ROLE,
                format!("未知的 agent_role：{role}"),
            ));
        }
    }
    let written = state
        .set_conversation_active_state(
            patch.conversation_id.clone(),
            patch.agent_role.as_deref(),
            patch.session_mode.as_deref(),
            patch.permission_level.as_deref(),
        )
        .await;
    if !written {
        return Err(not_found(
            error_codes::CONVERSATION_NOT_FOUND,
            "会话不存在或已过期",
        ));
    }
    let seed = load_seed(&state, &patch.conversation_id).await?;
    let cfg = state.cfg.read().await;
    // 脏的历史串按「未指定」处理（与 `GET /permission` 同取向）：只读投影不该因一行脏数据 500。
    let persisted_level =
        parse_optional_permission_level(seed.persisted_active_permission_level.as_deref())
            .unwrap_or(None);
    let permission = build_permission_view(PermissionViewInput {
        cfg: &cfg,
        requested_agent_role: seed.persisted_active_agent_role.as_deref(),
        persisted_agent_role: seed.persisted_active_agent_role.as_deref(),
        persisted_session_mode: seed.persisted_active_session_mode.as_deref(),
        persisted_level,
    });
    Ok(Json(ConversationStateView {
        conversation_id: patch.conversation_id,
        active_permission_level: seed.persisted_active_permission_level,
        active_session_mode: seed.persisted_active_session_mode,
        active_agent_role: seed.persisted_active_agent_role,
        permission,
    }))
}

#[cfg(all(test, feature = "web"))]
#[path = "http_tests.rs"]
mod http_tests;
