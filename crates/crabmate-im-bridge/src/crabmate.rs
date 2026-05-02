//! 调用 CrabMate **`POST /chat`**、**`POST /workspace`** 等 Web API，与侧栏行为对齐。

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::Value;

const JSON_UTF8: &str = "application/json; charset=utf-8";

#[derive(Clone)]
pub struct CrabmateClient {
    base_url: String,
    bearer: String,
    http: reqwest::Client,
}

impl CrabmateClient {
    /// `base_url` 示例：`http://127.0.0.1:8080`（**不要**带末尾 `/chat`）。
    pub fn new(base_url: impl Into<String>, bearer: impl Into<String>) -> reqwest::Result<Self> {
        Ok(Self {
            base_url: trim_trailing_slash(base_url.into()),
            bearer: bearer.into(),
            http: reqwest::Client::builder()
                .use_rustls_tls()
                .timeout(std::time::Duration::from_secs(300))
                .build()?,
        })
    }

    pub async fn chat_plain(
        &self,
        message: impl AsRef<str>,
        conversation_id: Option<&str>,
    ) -> Result<ChatPlainResponse, CrabmateError> {
        let url = format!("{}/chat", self.base_url);
        let body = ChatRequestBody {
            message: message.as_ref().to_string(),
            conversation_id: conversation_id.map(String::from),
        };
        let mut headers = HeaderMap::new();
        let auth = format!("Bearer {}", self.bearer.trim());
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth)
                .map_err(|e| CrabmateError::InvalidHeader(e.to_string()))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON_UTF8));

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let preview = utf8_preview(&bytes, 512);
            return Err(CrabmateError::HttpStatus {
                status: status.as_u16(),
                body_preview: preview,
            });
        }

        let parsed: ChatResponseBody = serde_json::from_slice(&bytes).map_err(|e| {
            CrabmateError::Decode(format!(
                "invalid CrabMate /chat JSON: {e}; preview={}",
                utf8_preview(&bytes, 256)
            ))
        })?;
        Ok(ChatPlainResponse {
            reply: parsed.reply,
            conversation_id: parsed.conversation_id,
        })
    }

    /// **`POST /workspace`**：`{"path":"..."}`；`path` 为空串表示恢复默认（与 CrabMate Web handler 一致）。
    pub async fn set_workspace(&self, path: &str) -> Result<(), CrabmateError> {
        let url = format!("{}/workspace", self.base_url);
        let body = serde_json::json!({ "path": path });
        let mut headers = HeaderMap::new();
        let auth = format!("Bearer {}", self.bearer.trim());
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth)
                .map_err(|e| CrabmateError::InvalidHeader(e.to_string()))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON_UTF8));

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let preview = utf8_preview(&bytes, 512);
            return Err(CrabmateError::HttpStatus {
                status: status.as_u16(),
                body_preview: preview,
            });
        }

        let v: Value = serde_json::from_slice(&bytes).map_err(|e| {
            CrabmateError::Decode(format!(
                "invalid CrabMate /workspace JSON: {e}; preview={}",
                utf8_preview(&bytes, 256)
            ))
        })?;
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            let err = v
                .get("error")
                .and_then(|x| x.as_str())
                .unwrap_or("workspace set failed");
            return Err(CrabmateError::Decode(err.to_string()));
        }
        Ok(())
    }
}

fn trim_trailing_slash(mut s: String) -> String {
    while s.ends_with('/') {
        s.pop();
    }
    s
}

fn utf8_preview(bytes: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    let t = s.trim();
    if t.len() <= max {
        t.to_string()
    } else {
        format!("{}…", &t[..max])
    }
}

#[derive(Debug, Serialize)]
struct ChatRequestBody {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ChatResponseBody {
    reply: String,
    conversation_id: String,
}

#[derive(Debug, Clone)]
pub struct ChatPlainResponse {
    pub reply: String,
    pub conversation_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CrabmateError {
    #[error("HTTP client: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid Authorization header value: {0}")]
    InvalidHeader(String),
    #[error("CrabMate returned HTTP {status}: {body_preview}")]
    HttpStatus { status: u16, body_preview: String },
    #[error("{0}")]
    Decode(String),
}
