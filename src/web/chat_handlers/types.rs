//! Web chat / upload / changelog / config 等 JSON 请求与响应体。

#[derive(serde::Deserialize)]
pub(crate) struct ChatRequestBody {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) conversation_id: Option<String>,
    /// 新建会话（无 `conversation_id` 或服务端尚无该 id）时选用命名角色；须与配置中角色 id 一致。已有会话时忽略。
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

/// Web：将会话在服务端截断到第 `before_user_ordinal` 条**普通**用户消息之前（0-based，与前端用户气泡序号一致）。
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

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示
#[derive(serde::Serialize)]
pub(crate) struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
}

/// `GET /workspace/changelog`：本会话工作区变更集 Markdown（与 **`session_workspace_changelist`** 注入正文同源）。
#[derive(serde::Deserialize)]
pub(crate) struct WorkspaceChangelogQuery {
    #[serde(default)]
    pub(crate) conversation_id: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct WorkspaceChangelogResponse {
    pub(crate) revision: u64,
    pub(crate) markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct ConfigReloadResponseBody {
    pub(crate) ok: bool,
    pub(crate) message: String,
}
