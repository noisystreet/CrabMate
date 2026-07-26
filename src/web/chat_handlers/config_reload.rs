//! `POST /config/reload`。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::web::app_state_facets::ConfigReloadFacet;
use crate::web::http_types::chat::{ApiError, ConfigReloadResponseBody};

/// 热重载 [`AgentConfig`] 可更字段（不含会话 SQLite 路径）；清空 MCP 进程缓存。
pub(crate) async fn config_reload_handler(
    State(facet): State<ConfigReloadFacet>,
) -> Result<Json<ConfigReloadResponseBody>, (StatusCode, Json<ApiError>)> {
    let path = facet.config_path_for_reload.as_deref();
    match crate::runtime::config_reload::reload_shared_agent_config(&facet.cfg, path).await {
        Ok(()) => Ok(Json(ConfigReloadResponseBody {
            ok: true,
            message: "配置已热重载。conversation_store_sqlite_path 与 reqwest Client 未重建；若变更 web_api_bearer_token 是否启用鉴权中间件，须重启 serve。web_api_require_bearer 与非空密钥的强制组合仅在下次启动 serve 时校验。".to_string(),
        })),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "CONFIG_RELOAD_FAILED",
                message: e,
                reason_code: None,
            }),
        )),
    }
}
