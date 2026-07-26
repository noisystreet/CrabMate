//! `serve` 静态挂载与受保护路由体积分层（根包 `build_app` 薄封装调用）。

use std::path::PathBuf;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// 受保护 JSON/multipart 路由共用请求体上限（字节）。
///
/// 须覆盖 **`POST /upload`** 单次请求总上限（上传逻辑允许约 200MiB 合计），略放大以容纳 multipart 边界开销。
pub const PROTECTED_API_BODY_LIMIT_BYTES: usize = 220 * 1024 * 1024;

pub fn layer_protected_body_limit<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(DefaultBodyLimit::max(PROTECTED_API_BODY_LIMIT_BYTES))
}

/// 挂载 `/uploads` 与（可选）SPA `fallback`（Leptos `dist`）。
pub fn mount_uploads_and_spa<S>(
    mut app: Router<S>,
    uploads_dir: PathBuf,
    static_dir: PathBuf,
    no_web: bool,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    app = app.nest_service(
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
            .service(ServeDir::new(uploads_dir)),
    );
    if !no_web {
        // axum 0.8+：禁止 `nest_service("/", …)`，未匹配 API/静态前缀的请求走 fallback。
        app = app.fallback_service(ServeDir::new(static_dir));
    }
    app
}
