//! `GET` / `PUT` / `POST` **`/user-data/*`**（受保护路由；密钥不落 GET 明文）。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::user_data::{
    LlmOverridesFile, McpServerStatusEntry, McpServersFile, McpServersFilePublic,
    McpServersImportResponse, McpServersStatusResponse, SecretsStatusResponse, UserPrefs,
    WebSessionsFile, WorkspaceListEntry,
};
use crate::user_data::{
    append_mcp_json_import, ensure_user_data_tree, list_workspaces, load_llm_overrides,
    load_mcp_servers_with_legacy_import, load_prefs, load_web_sessions,
    merge_mcp_commands_from_stored, normalize_mcp_servers_file, save_llm_overrides,
    save_mcp_servers, save_prefs, save_web_sessions, secrets_status, validate_sessions_value,
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

async fn mcp_legacy_import_params(state: &AppState) -> (bool, String, u64) {
    let cfg = state.http.cfg.read().await;
    (
        cfg.mcp_client.mcp_enabled,
        cfg.mcp_client.mcp_command.clone(),
        cfg.mcp_client.mcp_tool_timeout_secs,
    )
}

pub(crate) async fn get_mcp_servers_handler(
    State(state): State<Arc<AppState>>,
) -> Json<McpServersFilePublic> {
    let _ = ensure_user_data_tree();
    let (enabled, cmd, timeout) = mcp_legacy_import_params(&state).await;
    let file = load_mcp_servers_with_legacy_import(enabled, &cmd, timeout);
    Json(McpServersFilePublic::from(&file))
}

fn decode_mcp_servers_put(value: Value) -> Result<McpServersFile, String> {
    if let Ok(file) = serde_json::from_value::<McpServersFile>(value.clone())
        && !file.servers.is_empty()
    {
        return Ok(file);
    }
    if value.get("mcpServers").is_some() {
        let imported = crate::user_data::mcp_json_import::import_mcp_json_value(&value)?;
        let global_enabled = value
            .get("global_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let tool_timeout_secs = value
            .get("tool_timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(60)
            .max(1);
        return Ok(McpServersFile {
            schema_version: crate::user_data::SCHEMA_VERSION,
            global_enabled,
            tool_timeout_secs,
            servers: imported.entries,
        });
    }
    serde_json::from_value::<McpServersFile>(value).map_err(|e| {
        format!(
            "MCP 配置 JSON 无效（须为 CrabMate mcp_servers 或含 mcpServers 的 MCP 配置 JSON）: {e}"
        )
    })
}

pub(crate) async fn put_mcp_servers_handler(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let file =
        decode_mcp_servers_put(body).map_err(|e| user_data_err(StatusCode::BAD_REQUEST, e))?;
    let file = merge_mcp_commands_from_stored(file);
    let file =
        normalize_mcp_servers_file(file).map_err(|e| user_data_err(StatusCode::BAD_REQUEST, e))?;
    save_mcp_servers(&file).map_err(|e| user_data_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    crate::mcp::clear_mcp_process_cache().await;
    Ok(StatusCode::NO_CONTENT)
}

fn decode_mcp_json_import_body(body: Value) -> Result<Value, String> {
    if let Some(text) = body.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("JSON 为空".to_string());
        }
        return serde_json::from_str(trimmed).map_err(|e| format!("JSON 解析失败: {e}"));
    }
    Ok(body)
}

pub(crate) async fn post_mcp_servers_import_handler(
    Json(body): Json<Value>,
) -> Result<Json<McpServersImportResponse>, (StatusCode, String)> {
    let _ = ensure_user_data_tree();
    let value =
        decode_mcp_json_import_body(body).map_err(|e| user_data_err(StatusCode::BAD_REQUEST, e))?;
    let resp =
        append_mcp_json_import(&value).map_err(|e| user_data_err(StatusCode::BAD_REQUEST, e))?;
    crate::mcp::clear_mcp_process_cache().await;
    Ok(Json(resp))
}

pub(crate) async fn get_mcp_servers_status_handler(
    State(state): State<Arc<AppState>>,
) -> Json<McpServersStatusResponse> {
    let _ = ensure_user_data_tree();
    let (enabled, cmd, timeout) = mcp_legacy_import_params(&state).await;
    let file = load_mcp_servers_with_legacy_import(enabled, &cmd, timeout);
    let cfg = state.http.cfg.read().await;
    let resolved = crate::mcp::resolve_mcp_config(&cfg);
    let runtime = crate::mcp::mcp_servers_runtime_status(&resolved).await;
    let servers: Vec<McpServerStatusEntry> = runtime
        .into_iter()
        .map(|st| McpServerStatusEntry {
            id: st.id,
            name: st.name,
            slug: st.slug,
            enabled: st.enabled,
            connected: st.connected,
            openai_tool_names: st.openai_tool_names,
            remote_tools: st.remote_tools,
            last_error: st.last_error,
        })
        .collect();
    Json(McpServersStatusResponse {
        global_enabled: file.global_enabled,
        tool_timeout_secs: file.tool_timeout_secs,
        servers,
    })
}

pub(crate) async fn post_mcp_server_probe_handler(
    State(state): State<Arc<AppState>>,
    Path(server_id): Path<String>,
) -> Result<Json<McpServerStatusEntry>, (StatusCode, String)> {
    let (enabled, cmd, timeout) = mcp_legacy_import_params(&state).await;
    let _ = load_mcp_servers_with_legacy_import(enabled, &cmd, timeout);
    let cfg = state.http.cfg.read().await;
    let resolved = crate::mcp::resolve_mcp_config(&cfg);
    let Some(server) = resolved.servers.iter().find(|s| s.id == server_id) else {
        return Err(user_data_err(StatusCode::NOT_FOUND, "未找到 MCP 服务器"));
    };
    let st = crate::mcp::probe_mcp_server(server).await;
    Ok(Json(McpServerStatusEntry {
        id: st.id,
        name: st.name,
        slug: st.slug,
        enabled: st.enabled,
        connected: st.connected,
        openai_tool_names: st.openai_tool_names,
        remote_tools: st.remote_tools,
        last_error: st.last_error,
    }))
}

pub(crate) async fn post_mcp_servers_probe_all_handler(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<McpServerStatusEntry>> {
    let (enabled, cmd, timeout) = mcp_legacy_import_params(&state).await;
    let _ = load_mcp_servers_with_legacy_import(enabled, &cmd, timeout);
    let cfg = state.http.cfg.read().await;
    let resolved = crate::mcp::resolve_mcp_config(&cfg);
    let mut out = Vec::new();
    for server in resolved.servers.iter().filter(|s| s.enabled) {
        let st = crate::mcp::probe_mcp_server(server).await;
        out.push(McpServerStatusEntry {
            id: st.id,
            name: st.name,
            slug: st.slug,
            enabled: st.enabled,
            connected: st.connected,
            openai_tool_names: st.openai_tool_names,
            remote_tools: st.remote_tools,
            last_error: st.last_error,
        });
    }
    Json(out)
}
