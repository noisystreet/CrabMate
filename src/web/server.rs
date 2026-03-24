//! Web 服务路由组装（从 `lib.rs::run` 下沉）。

use axum::{
    Router,
    http::{HeaderValue, header},
    middleware,
    routing::{get, post},
};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

pub(crate) fn build_app(
    state: std::sync::Arc<crate::AppState>,
    no_web: bool,
    static_dir: std::path::PathBuf,
    uploads_dir_for_static: std::path::PathBuf,
) -> Router {
    let mut protected_api = Router::new()
        .route("/chat", post(crate::chat_handler))
        .route("/chat/stream", post(crate::chat_stream_handler))
        .route("/chat/approval", post(crate::chat_approval_handler))
        .route("/upload", post(crate::upload_handler))
        .route("/uploads/delete", post(crate::delete_uploads_handler))
        .route(
            "/workspace",
            get(crate::web::workspace::workspace_handler)
                .post(crate::web::workspace::workspace_set_handler),
        )
        .route(
            "/workspace/pick",
            get(crate::web::workspace::workspace_pick_handler),
        )
        .route(
            "/workspace/search",
            post(crate::web::workspace::workspace_search_handler),
        )
        .route(
            "/workspace/file",
            get(crate::web::workspace::workspace_file_read_handler)
                .post(crate::web::workspace::workspace_file_write_handler)
                .delete(crate::web::workspace::workspace_file_delete_handler),
        )
        .route(
            "/tasks",
            get(crate::web::task::tasks_get_handler).post(crate::web::task::tasks_set_handler),
        );
    if state.web_api_auth_enabled() {
        protected_api = protected_api.route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::require_web_api_bearer_auth,
        ));
    }
    let mut app = Router::new()
        .merge(protected_api)
        .route("/health", get(crate::health_handler))
        .route("/status", get(crate::status_handler))
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
