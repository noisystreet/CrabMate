//! `POST /chat/async` 与 **`GET /chat/jobs/{job_id}`** 的进程内任务状态（**非跨进程**；重启丢失）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::web::http_types::chat::ApiError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatAsyncJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Clone)]
#[allow(dead_code)] // `created_at` / webhook 元数据保留供运维扩展；当前仅轮询 `status`/`reply`/`error`。
pub(crate) struct ChatAsyncJobRecord {
    pub status: ChatAsyncJobStatus,
    pub conversation_id: String,
    pub created_at: Instant,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
    pub reply: Option<String>,
    pub conversation_revision: Option<u64>,
    pub error: Option<ApiError>,
}

pub(crate) type AsyncChatJobsMap = Arc<RwLock<HashMap<u64, ChatAsyncJobRecord>>>;

#[derive(Serialize)]
pub(crate) struct WebhookPayload<'a> {
    pub job_id: u64,
    pub status: &'a str,
    pub conversation_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'a ApiError>,
}
