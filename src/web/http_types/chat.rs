//! `POST /chat*`、`/upload*`、`POST /config/reload` 等 JSON 体；路由表见 [`crate::web::routes::chat::router`]。
//! 根级 [`ChatRequestBody`] 字段长度与条数上限见 [`super::validation`]。
//!
//! 真源在 [`crate::cm_api_contract`]；本文件按 handler 需要选择性再导出。

pub(crate) use crate::cm_api_contract::api::{
    ApiError, ConfigReloadResponseBody, DeleteUploadsBody, DeleteUploadsResponseBody,
    SessionConversationStoreRequestBody, SessionConversationStoreResponseBody, UploadResponseBody,
    UploadedFileInfo,
};
pub(crate) use crate::cm_api_contract::chat::{
    ChatApprovalRequestBody, ChatApprovalResponseBody, ChatAsyncRequestBody,
    ChatAsyncSubmitResponseBody, ChatBranchRequestBody, ChatBranchResponseBody,
    ChatJobStatusResponseBody, ChatRequestBody, ChatResponseBody, ClientLlmBody,
    ConversationMessagesQuery, ConversationMessagesResponseBody, ExecutorLlmBody, StreamResumeBody,
};

/// Web 会话快照响应（绑定运行时展示消息行）。
pub(crate) type ConversationMessagesHttpResponse = ConversationMessagesResponseBody<
    crate::runtime::message_snapshot_display::WebClientSnapshotMessage,
>;
