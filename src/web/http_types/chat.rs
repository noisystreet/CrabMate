//! `POST /chat*`、`/upload*`、`POST /config/reload` 等 JSON 体；路由表见 [`crate::web::routes::chat::router`]。

/// 用户对澄清问卷的作答；与 SSE `clarification_questionnaire.questionnaire_id` 及题目 `id` 对齐。
#[derive(serde::Deserialize, Clone)]
pub(crate) struct ClarifyQuestionnaireAnswersBody {
    pub(crate) questionnaire_id: String,
    /// 键为题目的 `id`，值为字符串（或 JSON 数字/布尔，服务端会规范为字符串）。
    #[serde(default)]
    pub(crate) answers: serde_json::Value,
}

#[derive(serde::Deserialize)]
pub(crate) struct ChatRequestBody {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) conversation_id: Option<String>,
    /// 命名角色 id（须与配置一致）。**新会话**建立首条 `system`；**已有会话**若与上次不同则刷新首条 `system` 并更新持久化 `active_agent_role`。
    #[serde(default, rename = "agent_role")]
    pub(crate) agent_role: Option<String>,
    #[serde(default)]
    pub(crate) approval_session_id: Option<String>,
    /// 覆盖本回合 `chat/completions` 的 **`temperature`**（0～2）；省略则用服务端配置。
    #[serde(default)]
    pub(crate) temperature: Option<f64>,
    /// 写入请求 JSON 的整数 **`seed`**（OpenAI 兼容）；与 `seed_policy: "omit"` 互斥。
    #[serde(default)]
    pub(crate) seed: Option<i64>,
    /// `omit` / `none`：本回合请求**不**带 `seed`（即使配置了默认 `llm_seed`）。
    #[serde(default)]
    pub(crate) seed_policy: Option<String>,
    /// 可选：浏览器侧覆盖本回合 LLM 网关 `api_base` / `model` / `api_key`（不写服务端配置）。
    #[serde(default)]
    pub(crate) client_llm: Option<ClientLlmBody>,
    /// 断线重连：挂接到进行中的 `job_id`；`after_seq` 与请求头 **`Last-Event-ID`** 取较大值后从环形缓冲重放。
    #[serde(default)]
    pub(crate) stream_resume: Option<StreamResumeBody>,
    /// 客户端实现的 SSE 控制面版本（与 `crabmate_sse_protocol::SSE_PROTOCOL_VERSION` 对齐）。省略表示不声明，服务端不据此拒绝；若 **大于** 服务端版本则 **400**（`SSE_CLIENT_TOO_NEW`）。
    #[serde(default, rename = "client_sse_protocol")]
    pub(crate) client_sse_protocol: Option<u8>,
    /// 本回合附带的图片 URL 列表（须为先前 `POST /upload` 返回的 **`/uploads/...`** 相对路径）；服务端组装 OpenAI 兼容多模态 `user.content`。
    #[serde(default)]
    pub(crate) image_urls: Vec<String>,
    /// 可选：回应上一轮 SSE **`clarification_questionnaire`**；合并进本回合 user 正文（在 `@` 文件引用展开之后）。
    #[serde(default)]
    pub(crate) clarify_questionnaire_answers: Option<ClarifyQuestionnaireAnswersBody>,
}

#[derive(serde::Deserialize)]
pub(crate) struct StreamResumeBody {
    pub(crate) job_id: u64,
    /// 已收到的最大 SSE `id`（无则 0）；可与 `Last-Event-ID` 合并取 max。
    #[serde(default)]
    pub(crate) after_seq: Option<u64>,
}

/// `ChatRequestBody::client_llm` 的 JSON 形状（与前端 `client_llm` 对象一致）。
#[derive(serde::Deserialize, Default)]
pub(crate) struct ClientLlmBody {
    #[serde(default)]
    pub(crate) api_base: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
}

#[derive(serde::Deserialize)]
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
#[derive(serde::Serialize)]
pub(crate) struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
}

#[derive(serde::Serialize)]
pub(crate) struct ConfigReloadResponseBody {
    pub(crate) ok: bool,
    pub(crate) message: String,
}
