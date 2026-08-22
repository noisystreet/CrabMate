//! `POST /chat*` 等 JSON 体（不含依赖运行时快照类型的会话消息响应）。

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use crate::cm_api_contract::api::ApiError;
use crate::cm_api_contract::chat_keys::{
    reject_unknown_async_chat_body_keys, reject_unknown_chat_body_keys,
};

fn schema_open_json_object(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": true,
        "description": "键为题目的 id，值为字符串（或 JSON 数字/布尔，服务端会规范为字符串）。"
    })
}

fn schema_session_mode(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": ["string", "null"],
        "enum": ["ask", "plan", "act"],
        "description": "Session capability mode (orthogonal to agent_role). ask/plan → readonly tools; act → full tools ∩ role allowlist. Default from config default_session_mode."
    })
}

fn schema_open_object_array(_gen: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": true
        },
        "description": "OpenAI 兼容 chat messages 对象数组"
    })
}

/// 用户对澄清问卷的作答；与 SSE `clarification_questionnaire.questionnaire_id` 及题目 `id` 对齐。
#[derive(Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClarifyQuestionnaireAnswersBody {
    pub questionnaire_id: String,
    /// 键为题目的 `id`，值为字符串（或 JSON 数字/布尔，服务端会规范为字符串）。
    #[serde(default)]
    #[schemars(schema_with = "schema_open_json_object")]
    pub answers: serde_json::Value,
}

/// 同步/流式对话共有字段。顶层 JSON 键白名单见 [`super::chat_keys`]；
/// 未知顶层键在自定义 [`Deserialize`] 中拒绝。
pub struct ChatRequestBody {
    pub message: String,
    pub conversation_id: Option<String>,
    pub agent_role: Option<String>,
    /// 本回合会话工作模式：`ask` / `plan` / `act`。
    pub session_mode: Option<String>,
    pub approval_session_id: Option<String>,
    pub temperature: Option<f64>,
    pub seed: Option<i64>,
    pub seed_policy: Option<String>,
    pub client_llm: Option<ClientLlmBody>,
    pub executor_llm: Option<ExecutorLlmBody>,
    pub readonly_tool_ttl_cache_secs: Option<u64>,
    pub stream_resume: Option<StreamResumeBody>,
    pub client_sse_protocol: Option<u8>,
    pub image_urls: Vec<String>,
    pub clarify_questionnaire_answers: Option<ClarifyQuestionnaireAnswersBody>,
}

/// `POST /chat/async`：与 [`ChatRequestBody`] 同形，另可选 `webhook_url` / `webhook_secret`。
pub struct ChatAsyncRequestBody {
    pub chat: ChatRequestBody,
    /// 非空时：任务进入 **`completed`** / **`failed`** 后向该 URL **POST** JSON。
    pub webhook_url: Option<String>,
    /// 可选：与 Webhook 一并发送 **`X-Crabmate-Webhook-Secret`**（**勿**在日志中输出完整值）。
    pub webhook_secret: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ChatAsyncSubmitResponseBody {
    pub job_id: u64,
    /// 初始状态恒为 **`pending`**。
    pub status: &'static str,
    pub conversation_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ChatJobStatusResponseBody {
    pub job_id: u64,
    pub status: String,
    pub conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamResumeBody {
    pub job_id: u64,
    /// 已收到的最大 SSE `id`（无则 0）；可与 `Last-Event-ID` 合并取 max。
    #[serde(default)]
    pub after_seq: Option<u64>,
}

/// `ChatRequestBody::client_llm` 的 JSON 形状（与前端 `client_llm` 对象一致）。
#[derive(Deserialize, Default, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientLlmBody {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// 可选：模型上下文窗口 token 上限（输入+输出），仅本回合。
    #[serde(default)]
    pub llm_context_tokens: Option<u64>,
    /// 可选：本回合覆盖供应商 **`thinking`** 相关开关；**`server`** / 省略表示跟随服务端配置。
    #[serde(default)]
    pub llm_thinking_mode: Option<String>,
}

/// `ChatRequestBody::executor_llm` 的 JSON 形状（与前端 `executor_llm` 对象一致）。
#[derive(Deserialize, Default, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutorLlmBody {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// `GET /conversation/messages` 响应中的 tiktoken 快照（OpenAPI / HTTP 契约）。
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct TiktokenPromptTokensOpenApi {
    pub prompt_tokens: u32,
    pub tiktoken_model: String,
}

/// 与 Client `CURRENT_LAYOUT_SCHEMA_VERSION`（Web 块布局 **2**）对齐；有元数据时写入 [`ConversationLayoutMeta`]。
pub const CONVERSATION_LAYOUT_SCHEMA_VERSION_V2: u32 = 2;

/// 单条 canonical 段（B2/E2 可选 hydration 键；旧会话可整段省略）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ConversationLayoutSegment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub segment_id: String,
    pub segment_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_tool_call_id: Option<String>,
    pub sequence: u32,
}

/// `GET /conversation/messages` 可选布局元数据。
///
/// **会话级**（与 `revision` 同范围），**不**随 `limit` / `before_index` 对 `messages` 分页切片。
/// 落盘 JSON 与本类型 serde 相同。省略该对象时客户端走 legacy hydration（B3 前官方 Web 亦不消费本字段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ConversationLayoutMeta {
    pub layout_schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<ConversationLayoutSegment>,
}

/// `GET /conversation/messages` 响应 OpenAPI 形状（`messages` 为 OpenAI 兼容对象数组）。
#[derive(Serialize, JsonSchema)]
pub struct ConversationMessagesResponseBodyOpenApi {
    pub conversation_id: String,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_agent_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiktoken_prompt_tokens: Option<TiktokenPromptTokensOpenApi>,
    /// 会话级布局；省略表示未持久化。不随本页 `messages` 窗口切片。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<ConversationLayoutMeta>,
    #[schemars(schema_with = "schema_open_object_array")]
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub total_count: u32,
    #[serde(default)]
    pub window_start_index: u32,
    #[serde(default)]
    pub has_older: bool,
}

/// `POST /chat/async` OpenAPI 形状：与 [`ChatRequestBodyWire`] 同形扁平 JSON + 可选 webhook 字段。
#[derive(JsonSchema)]
#[allow(dead_code)]
pub struct ChatAsyncRequestBodyOpenApi {
    #[schemars(flatten)]
    chat: ChatRequestBodyWire,
    /// 非空时：任务进入 `completed` / `failed` 后向该 URL POST JSON（须 http/https）。
    webhook_url: Option<String>,
    /// 可选：Webhook 请求头 `X-Crabmate-Webhook-Secret`（勿在日志输出完整值）。
    webhook_secret: Option<String>,
}

/// `POST /chat` / `POST /chat/stream` 请求的 JSON 线型（OpenAPI 与 [`ChatRequestBody`] 反序列化同源）。
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatRequestBodyWire {
    pub message: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default, rename = "agent_role")]
    pub agent_role: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema_session_mode")]
    pub session_mode: Option<String>,
    #[serde(default)]
    pub approval_session_id: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub seed_policy: Option<String>,
    #[serde(default)]
    pub client_llm: Option<ClientLlmBody>,
    #[serde(default)]
    pub executor_llm: Option<ExecutorLlmBody>,
    #[serde(default)]
    pub readonly_tool_ttl_cache_secs: Option<u64>,
    #[serde(default)]
    pub stream_resume: Option<StreamResumeBody>,
    #[serde(default, rename = "client_sse_protocol")]
    pub client_sse_protocol: Option<u8>,
    #[serde(default)]
    pub image_urls: Vec<String>,
    #[serde(default)]
    pub clarify_questionnaire_answers: Option<ClarifyQuestionnaireAnswersBody>,
}

impl From<ChatRequestBodyWire> for ChatRequestBody {
    fn from(s: ChatRequestBodyWire) -> Self {
        ChatRequestBody {
            message: s.message,
            conversation_id: s.conversation_id,
            agent_role: s.agent_role,
            session_mode: s.session_mode,
            approval_session_id: s.approval_session_id,
            temperature: s.temperature,
            seed: s.seed,
            seed_policy: s.seed_policy,
            client_llm: s.client_llm,
            executor_llm: s.executor_llm,
            readonly_tool_ttl_cache_secs: s.readonly_tool_ttl_cache_secs,
            stream_resume: s.stream_resume,
            client_sse_protocol: s.client_sse_protocol,
            image_urls: s.image_urls,
            clarify_questionnaire_answers: s.clarify_questionnaire_answers,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatApprovalRequestBody {
    pub approval_session_id: String,
    pub decision: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ChatApprovalResponseBody {
    pub ok: bool,
}

/// Web：将会话在服务端截断到第 `before_user_ordinal` 条**普通**用户消息之前。
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatBranchRequestBody {
    pub conversation_id: String,
    /// 从此序号对应的用户消息起（含）全部丢弃。
    pub before_user_ordinal: u64,
    /// 截断前客户端所知的 `revision`。
    pub expected_revision: u64,
}

#[derive(Serialize, JsonSchema)]
pub struct ChatBranchResponseBody {
    pub ok: bool,
    pub revision: u64,
}

#[derive(Serialize, JsonSchema)]
pub struct ChatResponseBody {
    pub reply: String,
    pub conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_revision: Option<u64>,
}

/// `GET /conversation/messages` 查询串。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationMessagesQuery {
    pub conversation_id: String,
    /// 分页：每页条数；省略或 `0` 表示返回过滤后的全量。
    #[serde(default)]
    pub limit: Option<u32>,
    /// 分页：取该下标**之前**的更早消息；省略表示取尾部一页。
    #[serde(default)]
    pub before_index: Option<u32>,
}

/// `GET /conversation/messages` 响应（消息行类型由调用方绑定）。
#[derive(serde::Serialize)]
pub struct ConversationMessagesResponseBody<M> {
    pub conversation_id: String,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_agent_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiktoken_prompt_tokens: Option<crate::cm_types::TiktokenPromptTokensSnapshot>,
    /// 会话级布局元数据；省略表示未写入。不随本页 `messages` 窗口切片；当前保存路径仍不写（hydration 不变）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<ConversationLayoutMeta>,
    pub messages: Vec<M>,
    #[serde(default)]
    pub total_count: u32,
    #[serde(default)]
    pub window_start_index: u32,
    #[serde(default)]
    pub has_older: bool,
}

fn chat_request_body_from_json(v: serde_json::Value) -> Result<ChatRequestBody, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "expected JSON object".to_string())?;
    reject_unknown_chat_body_keys(obj)?;
    let inner: ChatRequestBodyWire = serde_json::from_value(v).map_err(|e| e.to_string())?;
    Ok(inner.into())
}

fn chat_async_request_body_from_json(v: serde_json::Value) -> Result<ChatAsyncRequestBody, String> {
    let mut map = match v.as_object().cloned() {
        Some(m) => m,
        None => return Err("expected JSON object".to_string()),
    };
    reject_unknown_async_chat_body_keys(&map)?;
    let webhook_url = take_async_webhook_string(&mut map, "webhook_url")?;
    let webhook_secret = take_async_webhook_string(&mut map, "webhook_secret")?;
    let chat_val = serde_json::Value::Object(map);
    let inner: ChatRequestBodyWire = serde_json::from_value(chat_val).map_err(|e| e.to_string())?;
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
