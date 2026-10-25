//! `GET /permission`：权限等级与六轴投影的只读出口（handler 在 [`crate::web::permission`]）。
//!
//! 挂在**受保护** API 面（与 `/status` 不同）：它暴露上限来源与角色上限，属于运维/授权信息，
//! 不该在未配 Bearer 的部署里对任意来源可读。

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::AppState;
use crate::web::permission::permission_handler;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/permission", get(permission_handler))
}
