//! 共用 API 错误与上传 / 配置热重载 JSON 体。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
    /// 与 `code` 配套的细分子码（如 `INTERNAL_ERROR` 时的截断内部摘要）；旧客户端可忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// 与响应头 `x-request-id` 同值（有则填）；旧客户端可忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// 字段级 / 约束级细节；旧客户端可忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ApiErrorDetail>>,
}

/// 字段级或约束级错误细节（与顶层 `code` 同风格的大写蛇形子码）。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ApiErrorDetail {
    /// 稳定机器可读子码（建议 `INVALID_*` 等，与顶层 `code` 一致风格）。
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            reason_code: None,
            request_id: None,
            details: None,
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
            request_id: None,
            details: None,
        }
    }

    /// 附带与响应头 `x-request-id` 对齐的关联 id。
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// 有则写入 `request_id`，无则保持不变。
    pub fn with_request_id_opt(self, request_id: Option<String>) -> Self {
        match request_id {
            Some(id) => self.with_request_id(id),
            None => self,
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct UploadedFileInfo {
    pub url: String,
    pub filename: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Serialize, JsonSchema)]
pub struct UploadResponseBody {
    pub files: Vec<UploadedFileInfo>,
}

#[derive(Serialize, JsonSchema)]
pub struct DeleteUploadsResponseBody {
    pub deleted: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteUploadsBody {
    pub urls: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ConfigReloadResponseBody {
    pub ok: bool,
    pub message: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionConversationStoreRequestBody {
    pub sqlite: bool,
}

#[derive(Serialize)]
pub struct SessionConversationStoreResponseBody {
    pub ok: bool,
    pub message: String,
}
