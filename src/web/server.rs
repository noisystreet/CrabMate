//! Web 服务路由组装（从 `lib.rs::run` 下沉）。

use axum::{
    Router,
    http::{HeaderValue, header},
    middleware,
    routing::get,
};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// `web_api_bearer_layer_enabled`：启动时是否对受保护 API 挂 Web API 鉴权中间件（`Authorization: Bearer` / `X-API-Key`；热重载改密钥后须重启 `serve` 才能切换该层）。
/// 另见 **`AgentConfig::web_api_require_bearer`**：为 true 时 **`lib::run` 的 `serve` 分支**在启动前强制 **`web_api_bearer_token` 非空**，避免无意以匿名方式暴露 `/chat*` 等。
pub(crate) fn build_app(
    state: std::sync::Arc<crate::AppState>,
    no_web: bool,
    static_dir: std::path::PathBuf,
    uploads_dir_for_static: std::path::PathBuf,
    web_api_bearer_layer_enabled: bool,
) -> Router {
    let mut protected_api = Router::new()
        .merge(super::routes::chat::router())
        .merge(super::routes::workspace::router())
        .merge(super::routes::tasks::router())
        .merge(super::routes::config::router());
    if web_api_bearer_layer_enabled {
        protected_api = protected_api.route_layer(middleware::from_fn_with_state(
            state.clone(),
            super::chat_handlers::require_web_api_bearer_auth,
        ));
    }
    let mut app = Router::new()
        .merge(protected_api)
        .route("/openapi.json", get(super::openapi::openapi_json_handler))
        .merge(super::routes::system::router())
        .nest_service(
            "/uploads",
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::HeaderName::from_static("cross-origin-resource-policy"),
                    HeaderValue::from_static("same-site"),
                ))
                .service(ServeDir::new(uploads_dir_for_static)),
        );
    if !no_web {
        app = app.nest_service("/", ServeDir::new(static_dir));
    }
    app.with_state(state)
}
