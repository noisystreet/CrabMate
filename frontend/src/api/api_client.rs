//! `ApiClient` trait：将前端所有 HTTP 调用抽象为接口，以便未来 mock 测试。
//!
//! 生产实现为 [`RealApiClient`]（直接调用浏览器 `fetch`）。
//! 测试时可实现 `MockApiClient` 返回预设响应。

use async_trait::async_trait;

use crate::conversation_hydrate::ConversationMessagesResponse;
use crate::i18n::Locale;

use super::http::{
    ChatBranchError, StatusData, TasksData, UploadedFileInfo, WebUiConfig,
    WorkspaceChangelogResponse, WorkspaceData,
};

/// 前端 HTTP API 抽象。每个方法对应一个后端端点。
///
/// 默认实现委托给 `api::http` 中的自由函数（生产行为）。
/// 测试时可替换为返回预设响应的 mock 实现。
#[async_trait(?Send)]
#[allow(dead_code)]
pub trait ApiClient {
    // ── Workspace ──────────────────────────────────────────────

    async fn fetch_workspace(
        &self,
        path: Option<&str>,
        loc: Locale,
    ) -> Result<WorkspaceData, String>;

    async fn fetch_workspace_pick(&self, loc: Locale) -> Result<Option<String>, String>;

    async fn fetch_workspace_changelog(
        &self,
        conversation_id: Option<&str>,
        loc: Locale,
    ) -> Result<WorkspaceChangelogResponse, String>;

    async fn post_workspace_set(&self, path: Option<String>, loc: Locale)
    -> Result<String, String>;

    // ── Tasks ──────────────────────────────────────────────────

    async fn fetch_tasks(&self, loc: Locale) -> Result<TasksData, String>;

    async fn save_tasks(&self, data: &TasksData, loc: Locale) -> Result<TasksData, String>;

    // ── Status ─────────────────────────────────────────────────

    async fn fetch_status(&self, loc: Locale) -> Result<StatusData, String>;

    // ── Web UI Config ──────────────────────────────────────────

    async fn fetch_web_ui_config(&self, loc: Locale) -> Result<WebUiConfig, String>;

    // ── Upload ─────────────────────────────────────────────────

    async fn upload_files_multipart(
        &self,
        form: &web_sys::FormData,
        loc: Locale,
    ) -> Result<Vec<UploadedFileInfo>, String>;

    // ── Conversation / Branch ──────────────────────────────────

    async fn fetch_conversation_messages(
        &self,
        conversation_id: &str,
        loc: Locale,
    ) -> Result<ConversationMessagesResponse, String>;

    async fn post_chat_branch(
        &self,
        conversation_id: &str,
        before_user_ordinal: u64,
        expected_revision: u64,
        loc: Locale,
    ) -> Result<u64, ChatBranchError>;

    // ── Approval ───────────────────────────────────────────────

    async fn submit_chat_approval(
        &self,
        session_id: &str,
        decision: &str,
        loc: Locale,
    ) -> Result<(), String>;
}

/// 生产实现：直接委托 `api::http` 中的自由函数。
#[allow(dead_code)]
pub struct RealApiClient;

#[async_trait(?Send)]
impl ApiClient for RealApiClient {
    async fn fetch_workspace(
        &self,
        path: Option<&str>,
        loc: Locale,
    ) -> Result<WorkspaceData, String> {
        super::http::fetch_workspace(path, loc).await
    }

    async fn fetch_workspace_pick(&self, loc: Locale) -> Result<Option<String>, String> {
        super::http::fetch_workspace_pick(loc).await
    }

    async fn fetch_workspace_changelog(
        &self,
        conversation_id: Option<&str>,
        loc: Locale,
    ) -> Result<WorkspaceChangelogResponse, String> {
        super::http::fetch_workspace_changelog(conversation_id, loc).await
    }

    async fn post_workspace_set(
        &self,
        path: Option<String>,
        loc: Locale,
    ) -> Result<String, String> {
        super::http::post_workspace_set(path, loc).await
    }

    async fn fetch_tasks(&self, loc: Locale) -> Result<TasksData, String> {
        super::http::fetch_tasks(loc).await
    }

    async fn save_tasks(&self, data: &TasksData, loc: Locale) -> Result<TasksData, String> {
        super::http::save_tasks(data, loc).await
    }

    async fn fetch_status(&self, loc: Locale) -> Result<StatusData, String> {
        super::http::fetch_status(loc).await
    }

    async fn fetch_web_ui_config(&self, loc: Locale) -> Result<WebUiConfig, String> {
        super::http::fetch_web_ui_config(loc).await
    }

    async fn upload_files_multipart(
        &self,
        form: &web_sys::FormData,
        loc: Locale,
    ) -> Result<Vec<UploadedFileInfo>, String> {
        super::http::upload_files_multipart_raw(form, loc).await
    }

    async fn fetch_conversation_messages(
        &self,
        conversation_id: &str,
        loc: Locale,
    ) -> Result<ConversationMessagesResponse, String> {
        super::http::fetch_conversation_messages(conversation_id, loc).await
    }

    async fn post_chat_branch(
        &self,
        conversation_id: &str,
        before_user_ordinal: u64,
        expected_revision: u64,
        loc: Locale,
    ) -> Result<u64, ChatBranchError> {
        super::http::post_chat_branch(conversation_id, before_user_ordinal, expected_revision, loc)
            .await
    }

    async fn submit_chat_approval(
        &self,
        session_id: &str,
        decision: &str,
        loc: Locale,
    ) -> Result<(), String> {
        super::http::submit_chat_approval(session_id, decision, loc).await
    }
}
