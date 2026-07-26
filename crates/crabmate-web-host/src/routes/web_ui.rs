//! `GET /web-ui` 路由。

use axum::Router;
use axum::routing::get;

use crate::web_ui::web_ui_config_handler;

/// 无状态；可 merge 进任意 `Router<S>`。
pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/web-ui", get(web_ui_config_handler))
}
