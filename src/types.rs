//! API 与对话相关类型

use serde::{Deserialize, Serialize};

/// 拼接在 `api_base` 后的 OpenAI 兼容 chat 路径（无前导斜杠）。
pub const OPENAI_CHAT_COMPLETIONS_REL_PATH: &str = "chat/completions";

// ---------- 消息与请求 ----------

/// 对话消息（OpenAI 兼容格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// 无 `tool_calls` 的 `system` 消息（Web 首轮、CLI、TUI 空会话等共用）。
    pub fn system_only(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `user` 消息。
    pub fn user_only(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }
}

/// Web `/chat`、队列任务与 CLI 单次问答共用的首轮：`[system, user]`。
pub fn messages_chat_seed(system_prompt: &str, user_text: &str) -> Vec<Message> {
    vec![
        Message::system_only(system_prompt.to_string()),
        Message::user_only(user_text.to_string()),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// 工具定义（传给 API）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub typ: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// 请求体
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// ---------- 非流式响应（`stream: false` 时 chat/completions 返回体） ----------

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: Message,
    #[serde(default)]
    pub finish_reason: String,
}

// ---------- 流式 chunk ----------

/// 流式 chunk 中的 delta（OpenAI 兼容）
#[derive(Debug, Default, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    #[allow(dead_code)]
    pub role: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub struct StreamToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub typ: Option<String>,
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    #[allow(dead_code)]
    pub index: u32,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Option<Vec<StreamChoice>>,
}

// TUI 中用于“人工审批”的决策结果：拒绝/允许一次/永久允许。
#[derive(Clone, Copy, Debug)]
pub enum CommandApprovalDecision {
    Deny,
    AllowOnce,
    AllowAlways,
}

/// 流式输出被用户中止时 `stream_chat` 返回的 `finish_reason` 占位（非上游 API 原义）。
pub const USER_CANCELLED_FINISH_REASON: &str = "user_cancelled";

/// `complete_chat_retrying` 在用户取消时返回的错误消息（与 `run_agent_turn_common` 识别一致）。
pub const LLM_CANCELLED_ERROR: &str = "已取消";
