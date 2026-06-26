//! 本机用户数据目录（`~/.local/share/crabmate`）：prefs、按工作区 Web 会话、LLM 覆盖与 secrets。
//!
//! 磁盘读写与类型定义在 **`crabmate-internal`**；Web 请求体合并（依赖 `web::http_types`）保留在根 crate。

mod merge;

pub use crabmate_internal::user_data::*;
pub use merge::{merge_client_llm_body, merge_executor_llm_body};
