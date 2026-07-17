//! 浏览器 `fetch` + `/chat/stream` SSE 解析（单前端实现）。
//!
//! 子模块划分：[`browser`] 共享句柄、[`client_llm_storage`] 本机模型键值、[`http`] JSON API、[`chat_stream`] 流式聊天。

#![allow(clippy::collapsible_if)]

mod browser;
mod chat_stream;
pub(crate) mod client_llm_cache;
pub(crate) mod client_llm_storage;
mod http;
mod saved_models;
mod session_store;
pub mod user_data;

pub use chat_stream::{ChatStreamCallbacks, OnToolCallFn, SendChatStreamParams, send_chat_stream};
pub use client_llm_storage::{
    clear_client_llm_api_key_storage, clear_executor_llm_api_key_storage,
    client_llm_storage_has_api_key, executor_llm_storage_has_api_key,
    load_client_llm_text_fields_from_storage, load_executor_llm_text_fields_from_storage,
    load_readonly_tool_ttl_cache_follow_server_from_storage, persist_client_llm_to_storage,
    persist_executor_llm_to_storage, persist_readonly_tool_ttl_cache_follow_server,
};
#[allow(unused_imports)]
pub use http::{
    ChatBranchError, GithubRepoContextData, StatusData, TaskItem, TasksData, UploadedFileInfo,
    WebUiConfig, WorkspaceChangelogResponse, WorkspaceData, WorkspaceEntry, WorkspaceFileReadData,
    delete_workspace_dir, delete_workspace_file, fetch_conversation_messages,
    fetch_github_repo_context, fetch_status, fetch_tasks, fetch_web_ui_config, fetch_workspace,
    fetch_workspace_changelog, fetch_workspace_file, post_chat_branch, post_workspace_dir,
    post_workspace_file_write, post_workspace_file_write_opts, post_workspace_set, save_tasks,
    submit_chat_approval, upload_files_multipart,
};
pub use saved_models::{
    ExecutorLlmDraftSignals, MainLlmDraftSignals, SavedModelPreset,
    apply_saved_model_preset_to_executor_fields, apply_saved_model_preset_to_main_fields,
    load_saved_model_presets_from_storage, matching_saved_preset_index,
    persist_saved_model_presets_to_storage,
};
pub use session_store::post_session_conversation_store;
