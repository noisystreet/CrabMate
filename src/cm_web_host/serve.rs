//! `serve` 受保护路由体积分层（根包 `build_app` 薄封装调用）。
//!
//! `serve` 永远纯 API，不托管 SPA/静态文件；UI 由 Client 仓自行托管（CORS 见 `web_cors_allowed_origins`）。

use axum::Router;
use axum::extract::DefaultBodyLimit;

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
