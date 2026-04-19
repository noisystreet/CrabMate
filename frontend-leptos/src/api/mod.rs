//! 浏览器 `fetch` + `/chat/stream` SSE 解析（单前端实现）。
//!
//! 子模块划分：[`browser`] 共享句柄、[`client_llm_storage`] 本机模型键值、[`http`] JSON API、[`chat_stream`] 流式聊天。

#![allow(clippy::collapsible_if)]

mod browser;
mod chat_stream;
mod client_llm_storage;
mod http;

pub use chat_stream::{ChatStreamCallbacks, send_chat_stream};
pub use client_llm_storage::{
    clear_client_llm_api_key_storage, clear_executor_llm_api_key_storage,
    client_llm_storage_has_api_key, executor_llm_storage_has_api_key,
    load_client_llm_text_fields_from_storage, load_executor_llm_text_fields_from_storage,
    persist_client_llm_to_storage, persist_executor_llm_to_storage,
};
pub use http::{
    ChatBranchError, StatusData, TaskItem, TasksData, WorkspaceData, WorkspaceEntry,
    fetch_conversation_messages, fetch_status, fetch_tasks, fetch_web_ui_config, fetch_workspace,
    fetch_workspace_changelog, fetch_workspace_pick, post_chat_branch, post_workspace_set,
    save_tasks, submit_chat_approval, upload_files_multipart,
};
