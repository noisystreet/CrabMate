//! e2e 真实 LLM 测试公共辅助库。
//!
//! - [`test_server`]：`TestServer` 启动与管理
//! - [`sse_stream`]：SSE 事件流解析
//! - [`error_classify`]：错误自动分类与排障建议
//! - [`test_context`]：`TestContext` 失败时自动 dump 上下文

pub mod error_classify;
pub mod sse_stream;
pub mod test_server;

/// 放置在 `tests/` 目录下的测试用临时目录前缀。
pub const E2E_ARTIFACTS_ROOT: &str = ".crabmate/e2e_artifacts";
