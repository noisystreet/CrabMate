//! Chat 请求体、非流式/流式响应、`Tool` 定义及取消常量。

use serde::{Deserialize, Serialize};

use super::message::Message;

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

/// OpenAI 兼容 **`chat/completions`** 请求体的核心字段（模型、消息、工具与采样）。
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequestCore {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// OpenAI 兼容 **`seed`**；`None` 则 JSON 省略该字段（由供应商默认随机性决定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// 网关 / 厂商扩展字段（仍扁平序列化进同一 JSON 对象，见 [`ChatRequest`]）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ChatRequestVendorExtensions {
    /// MiniMax OpenAI 兼容扩展：为 `true` 时流式/非流式可将思维链与正文分离（`delta.reasoning_details` / `message.reasoning_details`）。
    #[serde(skip_serializing_if = "Option::is_none", rename = "reasoning_split")]
    pub reasoning_split: Option<bool>,
    /// 供应商扩展：**`thinking`**（如智谱 GLM-5 深度思考、Moonshot **kimi-k2.5** 开关）；由 **`llm_bigmodel_thinking`** / **`llm_kimi_thinking_disabled`** 等配置拼装（见 `docs/配置说明.md`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    /// OpenAI 兼容 **`response_format`**（如 DeepSeek [JSON Output](https://api-docs.deepseek.com/zh-cn/guides/json_mode) 的 `{"type":"json_object"}`）；`None` 则省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
}

/// 发往供应商的 **`chat/completions`** 请求体（[`ChatRequestCore`] + [`ChatRequestVendorExtensions`]，`serde` 双层 **`flatten`** 保持原有 JSON 扁平形状）。
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    #[serde(flatten)]
    pub core: ChatRequestCore,
    #[serde(flatten)]
    pub vendor: ChatRequestVendorExtensions,
}

impl std::ops::Deref for ChatRequest {
    type Target = ChatRequestCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl std::ops::DerefMut for ChatRequest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
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
    /// 推理模型流式思维链（OpenAI/DeepSeek 多为 **`reasoning_content`**；Ollama 等对 Qwen 等常为 **`reasoning`**）。
    #[serde(default, alias = "reasoning")]
    pub reasoning_content: Option<String>,
    /// MiniMax 在 **`reasoning_split: true`** 时流式返回；元素常为 `{"text": "…"}`，`text` 多为**累积**全文。
    #[serde(default)]
    pub reasoning_details: Option<Vec<serde_json::Value>>,
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
    /// 部分 OpenAI 兼容流会省略 `index`；缺省按 0 处理，避免整段 SSE 解析失败导致无输出。
    #[serde(default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandApprovalDecision {
    Deny,
    AllowOnce,
    AllowAlways,
}

/// 流式输出被用户中止时 `stream_chat` 返回的 `finish_reason` 占位（非上游 API 原义）。
pub const USER_CANCELLED_FINISH_REASON: &str = "user_cancelled";

/// 用户取消时协作路径使用的错误消息（与 `llm::LlmCompleteError::Cancelled` / `RunAgentTurnError` 识别一致）。
pub const LLM_CANCELLED_ERROR: &str = "已取消";

/// `/chat/stream` 任务被取消且 SSE 仍可投递时，控制面 `SsePayload::Error` 的 **`code`**（与 `docs/SSE协议.md` 一致）。
pub const SSE_STREAM_CANCELLED_CODE: &str = "STREAM_CANCELLED";
