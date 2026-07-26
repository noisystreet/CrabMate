//! 共用 API 错误与上传 / 配置热重载 JSON 体。

use serde::{Deserialize, Serialize};

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示。
#[derive(Serialize, Clone)]
pub struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
    /// 与 `code` 配套的细分子码（如 `INTERNAL_ERROR` 时的截断内部摘要）；旧客户端可忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            reason_code: None,
        }
    }

    pub fn with_reason(
        code: &'static str,
        message: impl Into<String>,
        reason_code: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            reason_code: Some(reason_code.into()),
        }
    }
}

#[derive(Serialize)]
pub struct UploadedFileInfo {
    pub url: String,
    pub filename: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct UploadResponseBody {
    pub files: Vec<UploadedFileInfo>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteUploadsBody {
    pub urls: Vec<String>,
}

#[derive(Serialize)]
pub struct DeleteUploadsResponseBody {
    pub deleted: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Serialize)]
pub struct ConfigReloadResponseBody {
    pub ok: bool,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConversationStoreRequestBody {
    pub sqlite: bool,
}

#[derive(Serialize)]
pub struct SessionConversationStoreResponseBody {
    pub ok: bool,
    pub message: String,
}
