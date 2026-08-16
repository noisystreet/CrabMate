//! SSE / HTTP API 共用的 tiktoken prompt 统计快照（与 `agent::tiktoken_prompt_tokens` 计数逻辑对齐）。

/// 与 `GET /conversation/messages` 等 API 对齐的 tiktoken 统计快照。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TiktokenPromptTokensSnapshot {
    /// 近似 prompt token 数（**不含**本轮 `tools` 定义、**不含** `max_tokens` 预留）。
    pub prompt_tokens: u32,
    /// 实际传入 `tiktoken_rs::num_tokens_from_messages` 的模型 id（可能与配置 `model` 不同：回落时）。
    pub tiktoken_model: String,
}
