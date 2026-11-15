//! E2E 专用夹具路由：仅在环境变量 **`CM_E2E_FIXTURES=1`**（或 **`true`**）时挂载。
//!
//! **不得**在生产部署中启用；供 Victauri E2E 等写入可分页的持久化会话，无需真实 LLM。

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;

use crate::AppState;
use crate::types::{Message, MessageContent};
use crate::web::app_state_facets::E2eConversationFixtureFacet;
use crate::web::http_types::chat::ApiError;
use crate::web::normalize_client_conversation_id;

#[derive(Deserialize)]
struct E2eSeedMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct E2eSeedConversationBody {
    conversation_id: String,
    messages: Vec<E2eSeedMessage>,
    /// 为 true 时若 id 已存在则先删再写（仅 E2E）。
    #[serde(default)]
    replace: bool,
}

/// `CM_E2E_FIXTURES` 的取值规则（纯函数，便于测试）。
///
/// 只认 `1` / `true` / `TRUE` / `yes` / `YES`（去首尾空白）；其余一律关闭，**包括**
/// `"0"` / `"false"` / 空串 / 未设置 —— 「设了但设成假值」不等于开启，这条以前没有任何测试钉住。
pub(crate) fn fixtures_flag_enabled(raw: Option<&str>) -> bool {
    raw.is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

/// 读环境变量并交给 [`fixtures_flag_enabled`]；`std::env` 只在这里出现。
pub(crate) fn enabled_from_env() -> bool {
    fixtures_flag_enabled(std::env::var("CM_E2E_FIXTURES").ok().as_deref())
}

/// 按给定开关装配夹具路由：`false` 返回 `None`（`build_app` 据此不挂载）。
///
/// 开关作为参数而非函数内读 env，使「挂不挂」能直接在进程内测——读 env 的版本
/// 在 `cargo test` 里没法安全切换（edition 2024 的 `set_var` 是 `unsafe`，
/// 且同进程多测试并发读写环境变量属未定义行为）。
pub(crate) fn router(enabled: bool) -> Option<Router<Arc<AppState>>> {
    if !enabled {
        return None;
    }
    Some(Router::new().route(
        "/e2e/fixtures/conversation",
        post(seed_conversation_handler),
    ))
}

/// 生产装配入口：按 `CM_E2E_FIXTURES` 决定是否挂载。
pub(crate) fn router_from_env() -> Option<Router<Arc<AppState>>> {
    router(enabled_from_env())
}

async fn seed_conversation_handler(
    State(facet): State<E2eConversationFixtureFacet>,
    Json(body): Json<E2eSeedConversationBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let conversation_id =
        normalize_client_conversation_id(Some(&body.conversation_id)).map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new("INVALID_CONVERSATION_ID", msg)),
            )
        })?;
    let Some(cid) = conversation_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_CONVERSATION_ID",
                "conversation_id 不能为空".to_string(),
            )),
        ));
    };
    if body.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_BODY",
                "messages 不能为空".to_string(),
            )),
        ));
    }
    let messages: Vec<Message> = body
        .messages
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: Some(MessageContent::from(m.content.as_str())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        })
        .collect();
    if body.replace {
        facet.delete_conversation_record(&cid).await;
    }
    let outcome = facet
        .save_conversation_messages_if_revision(cid.clone(), messages, None, None, None, None, Vec::new())
        .await;
    match outcome {
        crate::SaveConversationOutcome::Saved => Ok(StatusCode::NO_CONTENT),
        crate::SaveConversationOutcome::Conflict => Err((
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "CONVERSATION_EXISTS",
                "会话已存在；可设 replace=true".to_string(),
            )),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures_flag_enabled, router};

    /// 开启值：`1` / `true` / `TRUE` / `yes` / `YES`，允许首尾空白。
    #[test]
    fn truthy_values_enable_the_flag() {
        for raw in ["1", "true", "TRUE", "yes", "YES", " 1 ", "\ttrue\n"] {
            assert!(fixtures_flag_enabled(Some(raw)), "应为开启：{raw:?}");
        }
    }

    /// 其余一律关闭 —— 这是「设了开关但设成假值」的那批，此前完全没有测试。
    #[test]
    fn other_values_disable_the_flag() {
        for raw in ["0", "false", "FALSE", "no", "NO", "", "   ", "2", "on", "enabled"] {
            assert!(!fixtures_flag_enabled(Some(raw)), "应为关闭：{raw:?}");
        }
        assert!(!fixtures_flag_enabled(None), "未设置应为关闭");
    }

    /// 挂载与否只看参数：`false` 必须拿到 `None`（`build_app` 据此不 merge 夹具路由）。
    #[test]
    fn router_mounts_only_when_enabled() {
        assert!(router(true).is_some());
        assert!(router(false).is_none());
    }
}
