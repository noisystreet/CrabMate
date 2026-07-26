//! GitHub 在线模式 HTTP 响应信封（`data` 由宿主填入工具层类型）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubApiResponse<T> {
    #[serde(flatten)]
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
