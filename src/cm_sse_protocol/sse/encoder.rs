//! SSE 编码器 trait：将 `SsePayload` 序列化为 SSE `data:` 行 JSON 字符串。
//!
//! 当前仅 AG-UI 协议（v2）编码器；`encode_message` 便捷函数始终使用默认编码器，调用点无需改造。

use std::sync::Arc;

use super::protocol::SsePayload;

/// SSE 编码器：将 `SsePayload` 序列化为 SSE `data:` 行 JSON 字符串。
pub trait SseEncoder: Send + Sync {
    fn encode(&self, payload: &SsePayload) -> String;
}

/// 全局默认编码器（v2 AG-UI）。
pub fn default_encoder() -> Arc<dyn SseEncoder> {
    Arc::new(super::encoder_v2::V2Encoder)
}
