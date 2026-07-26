//! `POST /chat*`、`/upload*`、`POST /config/reload` 等 JSON 体；路由表见 [`crate::web::routes::chat::router`]。
//! 根级 [`ChatRequestBody`] 字段长度与条数上限见 [`super::validation`]。
//!
//! 对话请求/响应主体与键白名单在 **`crabmate-web-host`**；本文件再导出，并保留依赖运行时快照的
//! [`ConversationMessagesResponseBody`]。

pub(crate) use crabmate_web_host::http_types::api::{
    ApiError, ConfigReloadResponseBody, DeleteUploadsBody, DeleteUploadsResponseBody,
    SessionConversationStoreRequestBody, SessionConversationStoreResponseBody, UploadResponseBody,
    UploadedFileInfo,
};
pub(crate) use crabmate_web_host::http_types::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBody,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBody, ChatResponseBody, ClientLlmBody,
    ConversationMessagesQuery, ExecutorLlmBody, StreamResumeBody,
};

/// `GET /conversation/messages?conversation_id=`：只读拉取服务端已落盘会话（供 Web 刷新后与存储对齐）。
#[derive(serde::Serialize)]
pub(crate) struct ConversationMessagesResponseBody {
    pub(crate) conversation_id: String,
    pub(crate) revision: u64,
    /// 与会话存储列 `active_agent_role` 一致；空串时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_agent_role: Option<String>,
    /// 与供应商出站 `messages` 对齐的 tiktoken prompt token 粗估；失败或未启用时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tiktoken_prompt_tokens: Option<crate::types::TiktokenPromptTokensSnapshot>,
    pub(crate) messages: Vec<crate::runtime::message_snapshot_display::WebClientSnapshotMessage>,
    /// 过滤后可见消息总数；全量模式与 `messages.len()` 一致。
    #[serde(default)]
    pub(crate) total_count: u32,
    /// 本页第一条在过滤后数组中的下标。
    #[serde(default)]
    pub(crate) window_start_index: u32,
    /// 是否还有更早消息可拉取。
    #[serde(default)]
    pub(crate) has_older: bool,
}
