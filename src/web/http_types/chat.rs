//! `POST /chat*`、`/upload*`、`POST /config/reload` 等 JSON 体；路由表见 [`crate::web::routes::chat::router`]。
//! 根级 [`ChatRequestBody`] 字段长度与条数上限见 [`super::validation`]。
//!
//! 对话请求/响应主体与键白名单在 **`crabmate-web-host`**；本文件再导出。

pub(crate) use crate::cm_web_host::http_types::api::{
    ApiError, ConfigReloadResponseBody, DeleteUploadsBody, DeleteUploadsResponseBody,
    SessionConversationStoreRequestBody, SessionConversationStoreResponseBody, UploadResponseBody,
    UploadedFileInfo,
};
pub(crate) use crate::cm_web_host::http_types::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBody,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBody, ChatResponseBody, ClientLlmBody,
    ConversationMessagesQuery, ConversationMessagesResponseBody, ExecutorLlmBody, StreamResumeBody,
};

/// Web 会话快照响应（绑定运行时展示消息行）。
pub(crate) type ConversationMessagesHttpResponse = ConversationMessagesResponseBody<
    crate::runtime::message_snapshot_display::WebClientSnapshotMessage,
>;
