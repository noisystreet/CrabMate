//! `GET` / `PUT` / `POST` **`/user-data/*`**（受保护路由；密钥不落 GET 明文）。

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::user_data::{
    LlmOverridesFile, SecretsStatusResponse, UserPrefs, WebSessionsFile, WorkspaceListEntry,
};
use crate::user_data::{
    ensure_user_data_tree, list_workspaces, load_llm_overrides, load_prefs, load_web_sessions,
    save_llm_overrides, save_prefs, save_web_sessions, secrets_status, validate_sessions_value,
    write_secret_client_llm, write_secret_executor_llm, write_secret_web_api_bearer,
};

#[derive(Debug, Deserialize)]
pub(crate) struct SecretWriteBody {
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PutSessionsBody {
    #[serde(default)]
    pub(crate) sessions: Value,
    #[serde(default)]
    pub(crate) active_session_id: Option<String>,
}

fn user_data_err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (status, msg.into())
}

pub(crate) async fn get_prefs_handler() -> Json<UserPrefs> {
    let _ = ensure_user_data_tree();
    Json(load_prefs())
}

pub(crate) async fn put_prefs_handler(
    Json(prefs): Json<UserPrefs>,
) -> Result<StatusCode, (StatusCode, String)> {
    save_prefs(&prefs).map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_llm_overrides_handler() -> Json<LlmOverridesFile> {
    let _ = ensure_user_data_tree();
    Json(load_llm_overrides())
}

pub(crate) async fn put_llm_overrides_handler(
    Json(file): Json<LlmOverridesFile>,
) -> Result<StatusCode, (StatusCode, String)> {
    save_llm_overrides(&file).map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_secrets_status_handler() -> Json<SecretsStatusResponse> {
    Json(secrets_status())
}

pub(crate) async fn put_secret_client_llm_handler(
    Json(body): Json<SecretWriteBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let key = body.api_key.unwrap_or_default();
    write_secret_client_llm(&key)
        .map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn put_secret_executor_llm_handler(
    Json(body): Json<SecretWriteBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let key = body.api_key.unwrap_or_default();
    write_secret_executor_llm(&key)
        .map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn put_secret_web_api_bearer_handler(
    Json(body): Json<SecretWriteBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = body.token.or(body.api_key).unwrap_or_default();
    write_secret_web_api_bearer(&token)
        .map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_workspaces_handler()
-> Result<Json<Vec<WorkspaceListEntry>>, (StatusCode, String)> {
    list_workspaces()
        .map(Json)
        .map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub(crate) async fn get_current_sessions_handler(
    State(state): State<Arc<AppState>>,
) -> Json<WebSessionsFile> {
    let ws = state.effective_workspace_path().await;
    Json(load_web_sessions(&ws))
}

pub(crate) async fn put_current_sessions_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutSessionsBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    validate_sessions_value(&body.sessions)
        .map_err(|e| user_data_err(StatusCode::BAD_REQUEST, e))?;
    let ws = state.effective_workspace_path().await;
    let file = WebSessionsFile {
        schema_version: crate::user_data::SCHEMA_VERSION,
        sessions: body.sessions,
        active_session_id: body.active_session_id,
    };
    save_web_sessions(&ws, &file)
        .map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}
