//! `GET /skills` JSON 体；路由表见 [`crate::web::routes::skills::router`]。
//!
//! 类型定义在 **`crabmate-web-host`**（阶段 B 首迁）；此处再导出保持 `crate::web::http_types::skills` 路径。

pub use crabmate_web_host::http_types::skills::{SkillListItem, SkillsListResponse};
