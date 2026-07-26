//! `/skills` 路由表；handler 在 [`crate::web::skills_handlers`]。

use axum::Router;
use axum::routing::get;

use crate::web::skills_handlers::skills_list_handler;

pub(crate) fn router() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new().route("/skills", get(skills_list_handler))
}
