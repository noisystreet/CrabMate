//! Web 服务路由组装：根包合并域路由；静态挂载与体积分层在 **`crabmate-web-host::serve`**。

use axum::Router;
use axum::middleware;
use axum::routing::get;

/// `web_api_bearer_layer_enabled`：启动时是否对受保护 API 挂 Web API 鉴权中间件。
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
    app = crabmate_web_host::serve::mount_uploads_and_spa(
        app,
        uploads_dir_for_static,
        static_dir,
        no_web,
    );
    // 最外层：所有响应带 `x-request-id`（含 401）；handler 可从 Extensions 取同值写入 ApiError。
    app.layer(middleware::from_fn(super::request_id::attach_request_id))
        .with_state(state)
}
