//! Web 服务路由组装（从 `lib.rs::run` 下沉）。

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// 受保护 JSON/multipart 路由共用请求体上限（字节）。
///
/// 须覆盖 **`POST /upload`** 单次请求总上限（上传逻辑允许约 200MiB 合计），略放大以容纳 multipart 边界开销。
const PROTECTED_API_BODY_LIMIT_BYTES: usize = 220 * 1024 * 1024;

/// `web_api_bearer_layer_enabled`：启动时是否对受保护 API 挂 Web API 鉴权中间件（`Authorization: Bearer` / `X-API-Key`；热重载改密钥后须重启 `serve` 才能切换该层）。
/// 另见 **`AgentConfig::web_api_require_bearer`**：为 **`true`** 时 **`serve`** 在启动前强制 **`web_api_bearer_token` 非空**；默认 **`false`** 允许先起服务，由非空密钥决定是否挂载 Bearer 中间件。
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
        .merge(super::routes::config::router())
        .merge(super::routes::user_data::router());
    if web_api_bearer_layer_enabled {
        protected_api = protected_api.route_layer(middleware::from_fn_with_state(
            state.clone(),
            super::chat_handlers::require_web_api_bearer_auth,
        ));
    }
    protected_api = protected_api.layer(DefaultBodyLimit::max(PROTECTED_API_BODY_LIMIT_BYTES));
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
