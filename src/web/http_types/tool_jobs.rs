//! `GET /tools/jobs/{id}`、`POST /tools/jobs/{id}/cancel` JSON 形状；路由见 [`crate::web::routes::tools::router`]。
//!
//! 类型定义在 [`crate::cm_api_contract::tool_jobs`]；handler 见 [`crate::web::tool_jobs`]。

pub use crate::cm_api_contract::tool_jobs::*;
