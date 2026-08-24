//! SSE / HTTP API 共用的 tiktoken prompt 统计快照（与 `agent::tiktoken_prompt_tokens` 计数逻辑对齐）。

/// 与 `GET /conversation/messages` 等 API 对齐的 tiktoken 统计快照。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TiktokenPromptTokensSnapshot {
    /// 兼容旧 Client 的消息 prompt Token；不含 tools 与输出预留。完整分母/分子见下方软字段。
    pub prompt_tokens: u32,
    /// 实际传入 `tiktoken_rs::num_tokens_from_messages` 的模型 id（可能与配置 `model` 不同：回落时）。
    pub tiktoken_model: String,
    /// Phase 3：最终请求的输入 Token 分项合计（messages + tools + attachments + vendor overhead）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_input_tokens: Option<u32>,
    /// 扣除输出预留与安全余量后，模型调用可用的统一输入预算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_schema_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_tokens: Option<u32>,
    /// `tiktoken` / `character_fallback` / `provider_usage`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counting_source: Option<String>,
    /// 供应商响应 usage 给出的输入 Token；仅在上游返回时存在，用于校准估算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_input_tokens: Option<u64>,
}
