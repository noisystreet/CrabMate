//! Web 服务路由组装：根包合并域路由；静态挂载与体积分层在 **`crabmate-web-host::serve`**。

use axum::Router;
use axum::middleware;
use axum::routing::get;

/// `web_api_bearer_layer_enabled`：启动时是否对受保护 API 挂 Web API 鉴权中间件。
/// `static_dir`：仅 `--with-web` 时传入已解析的 SPA 根；纯 API 传 `None`（不探测 dist）。
/// `cors_allowed_origins`：非空时在最外层挂 CORS 白名单（启动时装配，热更不改层）。
pub(crate) fn build_app(
    state: std::sync::Arc<crate::AppState>,
    static_dir: Option<std::path::PathBuf>,
    uploads_dir_for_static: std::path::PathBuf,
    web_api_bearer_layer_enabled: bool,
    cors_allowed_origins: Vec<String>,
) -> Router {
    let mut protected_api = Router::new()
        .merge(super::routes::chat::router())
        .merge(super::routes::workspace::router())
        .merge(super::routes::skills::router())
        .merge(super::routes::github::router())
        .merge(super::routes::tasks::router())
        .merge(super::routes::config::router())
        .merge(super::routes::user_data::router());
    if web_api_bearer_layer_enabled {
        protected_api = protected_api.route_layer(middleware::from_fn_with_state(
            state.clone(),
            super::chat_handlers::require_web_api_bearer_auth,
        ));
    }
    protected_api = crabmate_web_host::serve::layer_protected_body_limit(protected_api);
    let mut app = Router::new()
        .merge(protected_api)
        .route("/openapi.json", get(super::openapi::openapi_json_handler))
        .merge(super::routes::system::router());
    if let Some(e2e) = super::routes::e2e_fixtures::router() {
        app = app.merge(e2e);
    }
    let cors_layer = crabmate_web_host::try_cors_layer(&cors_allowed_origins);
    let allow_cross_origin_uploads = cors_layer.is_some();
    app = crabmate_web_host::serve::mount_uploads_and_spa(
        app,
        uploads_dir_for_static,
        static_dir,
        allow_cross_origin_uploads,
    );
    // 外层：`x-request-id`；再外层 CORS（若启用），以便预检 OPTIONS 不被其它层挡住。
    app = app.layer(middleware::from_fn(super::request_id::attach_request_id));
    if let Some(cors) = cors_layer {
        app = app.layer(cors);
    }
    app.with_state(state)
}
