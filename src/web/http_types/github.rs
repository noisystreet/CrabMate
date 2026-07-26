//! GitHub 在线模式 HTTP JSON 契约。
//!
//! 响应信封在 **`crabmate-web-host`**；`data` 类型仍来自工具层。

pub use crate::tools::web_api::{GithubPrCurrentChecksData, GithubRepoContextData};
pub use crabmate_web_host::http_types::github::GithubApiResponse;

pub type GithubRepoContextResponse = GithubApiResponse<GithubRepoContextData>;
pub type GithubPrCurrentChecksResponse = GithubApiResponse<GithubPrCurrentChecksData>;
