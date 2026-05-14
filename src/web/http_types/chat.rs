//! `POST /chat*`、`/upload*`、`POST /config/reload` 等 JSON 体；路由表见 [`crate::web::routes::chat::router`]。
//! 根级 [`ChatRequestBody`] 字段长度与条数上限见 [`super::validation`]。

use serde::Deserialize;

/// 用户对澄清问卷的作答；与 SSE `clarification_questionnaire.questionnaire_id` 及题目 `id` 对齐。
#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClarifyQuestionnaireAnswersBody {
    pub(crate) questionnaire_id: String,
    /// 键为题目的 `id`，值为字符串（或 JSON 数字/布尔，服务端会规范为字符串）。
    #[serde(default)]
    pub(crate) answers: serde_json::Value,
}

/// 同步/流式对话共有字段。顶层 JSON 键白名单见 [`super::validation::CHAT_REQUEST_BODY_ALLOWED_KEYS`]；
/// 未知顶层键在自定义 [`Deserialize`] 中拒绝。
pub(crate) struct ChatRequestBody {
    pub(crate) message: String,
    pub(crate) conversation_id: Option<String>,
    pub(crate) agent_role: Option<String>,
    pub(crate) approval_session_id: Option<String>,
    pub(crate) temperature: Option<f64>,
    pub(crate) seed: Option<i64>,
    pub(crate) seed_policy: Option<String>,
    pub(crate) client_llm: Option<ClientLlmBody>,
    pub(crate) executor_llm: Option<ExecutorLlmBody>,
    pub(crate) execution_mode: Option<String>,
    pub(crate) readonly_tool_ttl_cache_secs: Option<u64>,
    pub(crate) stream_resume: Option<StreamResumeBody>,
    pub(crate) client_sse_protocol: Option<u8>,
    pub(crate) image_urls: Vec<String>,
    pub(crate) clarify_questionnaire_answers: Option<ClarifyQuestionnaireAnswersBody>,
}

/// `POST /chat/async`：与 [`ChatRequestBody`] 同形，另可选 `webhook_url` / `webhook_secret`（自定义反序列化）。
pub(crate) struct ChatAsyncRequestBody {
    pub(crate) chat: ChatRequestBody,
    /// 非空时：任务进入 **`completed`** / **`failed`** 后向该 URL **POST** JSON（`Content-Type: application/json`）；须为 **http** 或 **https**。
    pub(crate) webhook_url: Option<String>,
    /// 可选：与 Webhook 一并发送 **`X-Crabmate-Webhook-Secret`**（集成方自行校验；**勿**在日志中输出完整值）。
    pub(crate) webhook_secret: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatAsyncSubmitResponseBody {
    pub(crate) job_id: u64,
    /// 初始状态恒为 **`pending`**（轮询见 **`GET /chat/jobs/{job_id}`**）。
    pub(crate) status: &'static str,
    pub(crate) conversation_id: String,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatJobStatusResponseBody {
    pub(crate) job_id: u64,
    pub(crate) status: String,
    pub(crate) conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<ApiError>,
}

#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamResumeBody {
    pub(crate) job_id: u64,
    /// 已收到的最大 SSE `id`（无则 0）；可与 `Last-Event-ID` 合并取 max。
    #[serde(default)]
    pub(crate) after_seq: Option<u64>,
}

/// `ChatRequestBody::client_llm` 的 JSON 形状（与前端 `client_llm` 对象一致）。
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientLlmBody {
    #[serde(default)]
    pub(crate) api_base: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    /// 可选：模型上下文窗口 token 上限（输入+输出），仅本回合；与 `llm_context_tokens` / `CM_LLM_CONTEXT_TOKENS` 一致。
    #[serde(default)]
    pub(crate) llm_context_tokens: Option<u64>,
    /// 可选：本回合覆盖供应商 **`thinking`** 相关开关（智谱 GLM、Moonshot kimi-k2.5 等）；**`server`** / 省略表示跟随服务端配置。
    #[serde(default)]
    pub(crate) llm_thinking_mode: Option<String>,
}

/// `ChatRequestBody::executor_llm` 的 JSON 形状（与前端 `executor_llm` 对象一致）。
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutorLlmBody {
    #[serde(default)]
    pub(crate) api_base: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
}

/// 与 [`ChatRequestBody`] 同形的 serde 助手（顶层键另由 [`crate::web::http_types::validation`] 校验）。
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatRequestBodySerde {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) conversation_id: Option<String>,
    #[serde(default, rename = "agent_role")]
    pub(crate) agent_role: Option<String>,
    #[serde(default)]
    pub(crate) approval_session_id: Option<String>,
    #[serde(default)]
    pub(crate) temperature: Option<f64>,
    #[serde(default)]
    pub(crate) seed: Option<i64>,
    #[serde(default)]
    pub(crate) seed_policy: Option<String>,
    #[serde(default)]
    pub(crate) client_llm: Option<ClientLlmBody>,
    #[serde(default)]
    pub(crate) executor_llm: Option<ExecutorLlmBody>,
    #[serde(default)]
    pub(crate) execution_mode: Option<String>,
    #[serde(default)]
    pub(crate) readonly_tool_ttl_cache_secs: Option<u64>,
    #[serde(default)]
    pub(crate) stream_resume: Option<StreamResumeBody>,
    #[serde(default, rename = "client_sse_protocol")]
    pub(crate) client_sse_protocol: Option<u8>,
    #[serde(default)]
    pub(crate) image_urls: Vec<String>,
    #[serde(default)]
    pub(crate) clarify_questionnaire_answers: Option<ClarifyQuestionnaireAnswersBody>,
}

impl From<ChatRequestBodySerde> for ChatRequestBody {
    fn from(s: ChatRequestBodySerde) -> Self {
        ChatRequestBody {
            message: s.message,
            conversation_id: s.conversation_id,
            agent_role: s.agent_role,
            approval_session_id: s.approval_session_id,
            temperature: s.temperature,
            seed: s.seed,
            seed_policy: s.seed_policy,
            client_llm: s.client_llm,
            executor_llm: s.executor_llm,
            execution_mode: s.execution_mode,
            readonly_tool_ttl_cache_secs: s.readonly_tool_ttl_cache_secs,
            stream_resume: s.stream_resume,
            client_sse_protocol: s.client_sse_protocol,
            image_urls: s.image_urls,
            clarify_questionnaire_answers: s.clarify_questionnaire_answers,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatApprovalRequestBody {
    pub(crate) approval_session_id: String,
    pub(crate) decision: String,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatApprovalResponseBody {
    pub(crate) ok: bool,
}

/// Web：将会话在服务端截断到第 `before_user_ordinal` 条**普通**用户消息之前（0-based，与前端用户气泡序号一致；不含未展示之注入类 `user`）。
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatBranchRequestBody {
    pub(crate) conversation_id: String,
    /// 从此序号对应的用户消息起（含）全部丢弃；例如 `1` 表示保留第 0 条用户及之前上下文。
    pub(crate) before_user_ordinal: u64,
    /// 截断前客户端所知的 `revision`（与冲突检测一致；可从最近一次成功回合推断）。
    pub(crate) expected_revision: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatBranchResponseBody {
    pub(crate) ok: bool,
    /// 截断成功后的 revision（与 `keep_message_count == 当前长度` 时也会递增一次的行为一致：仅当 SQLite/内存实际执行了 UPDATE）。
    pub(crate) revision: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct UploadedFileInfo {
    pub(crate) url: String,
    pub(crate) filename: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct UploadResponseBody {
    pub(crate) files: Vec<UploadedFileInfo>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteUploadsBody {
    pub(crate) urls: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct DeleteUploadsResponseBody {
    pub(crate) deleted: Vec<String>,
    pub(crate) skipped: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatResponseBody {
    pub(crate) reply: String,
    pub(crate) conversation_id: String,
    /// 写入存储后的 revision（供 `POST /chat/branch`）；无持久化会话时可能为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_revision: Option<u64>,
}

/// `GET /conversation/messages` 查询串。
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationMessagesQuery {
    pub(crate) conversation_id: String,
}

/// `GET /conversation/messages?conversation_id=`：只读拉取服务端已落盘会话（供 Web 刷新后与存储对齐）。
#[derive(serde::Serialize)]
pub(crate) struct ConversationMessagesResponseBody {
    pub(crate) conversation_id: String,
    pub(crate) revision: u64,
    /// 与会话存储列 `active_agent_role` 一致；空串时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_agent_role: Option<String>,
    pub(crate) messages: Vec<crate::types::Message>,
}

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示
#[derive(serde::Serialize, Clone)]
pub(crate) struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
    /// 与 `code` 配套的细分子码（如 `INTERNAL_ERROR` 时的截断内部摘要）；旧客户端可忽略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

impl ApiError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            reason_code: None,
        }
    }

    pub(crate) fn with_reason(
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

fn chat_request_body_from_json(v: serde_json::Value) -> Result<ChatRequestBody, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "expected JSON object".to_string())?;
    super::validation::reject_unknown_chat_body_keys(obj)?;
    let inner: ChatRequestBodySerde = serde_json::from_value(v).map_err(|e| e.to_string())?;
    Ok(inner.into())
}

fn chat_async_request_body_from_json(v: serde_json::Value) -> Result<ChatAsyncRequestBody, String> {
    let mut map = match v.as_object().cloned() {
        Some(m) => m,
        None => return Err("expected JSON object".to_string()),
    };
    super::validation::reject_unknown_async_chat_body_keys(&map)?;
    let webhook_url = take_async_webhook_string(&mut map, "webhook_url")?;
    let webhook_secret = take_async_webhook_string(&mut map, "webhook_secret")?;
    let chat_val = serde_json::Value::Object(map);
    let inner: ChatRequestBodySerde =
        serde_json::from_value(chat_val).map_err(|e| e.to_string())?;
    Ok(ChatAsyncRequestBody {
        chat: inner.into(),
        webhook_url,
        webhook_secret,
    })
}

fn take_async_webhook_string(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &'static str,
) -> Result<Option<String>, String> {
    match map.remove(key) {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("{key} 须为 JSON 字符串或省略")),
    }
}

impl<'de> Deserialize<'de> for ChatRequestBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        chat_request_body_from_json(v).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ChatAsyncRequestBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        chat_async_request_body_from_json(v).map_err(serde::de::Error::custom)
    }
}

#[derive(serde::Serialize)]
pub(crate) struct ConfigReloadResponseBody {
    pub(crate) ok: bool,
    pub(crate) message: String,
}

/// `POST /config/session/conversation-store`：在进程内切换 Web 会话存储后端（内存 ↔ SQLite）。
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionConversationStoreRequestBody {
    pub(crate) sqlite: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct SessionConversationStoreResponseBody {
    pub(crate) ok: bool,
    pub(crate) message: String,
}
