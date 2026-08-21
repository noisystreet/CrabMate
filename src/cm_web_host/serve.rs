//! `serve` 静态挂载与受保护路由体积分层（根包 `build_app` 薄封装调用）。

use std::path::PathBuf;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower_http::services::ServeDir;

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

/// 可选挂载 SPA `fallback`（Leptos `dist`）。
///
/// `static_dir`：仅在显式托管 UI（`serve --with-web`）时传入；`None` 表示纯 API，不挂 SPA。
/// 聊天附图走受保护 **`GET /uploads/{filename}`**，不再匿名静态挂载。
pub fn mount_uploads_and_spa<S>(mut app: Router<S>, static_dir: Option<PathBuf>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if let Some(dir) = static_dir {
        // axum 0.8+：禁止 `nest_service("/", …)`，未匹配 API/静态前缀的请求走 fallback。
        app = app.fallback_service(ServeDir::new(dir));
    }
    app
}
