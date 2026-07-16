//! SSE 编码器 trait：将 `SsePayload` 序列化为 SSE `data:` 行 JSON 字符串。
//!
//! 当前 v1 为当前协议格式；后续 v2（AG-UI）新增 `V2Encoder` 实现同一 trait。
//! `encode_message` 便捷函数始终使用当前 encoder，调用点无需改造。

use std::sync::Arc;

use super::protocol::SsePayload;

/// SSE 编码器：将 `SsePayload` 序列化为 SSE `data:` 行 JSON 字符串。
pub trait SseEncoder: Send + Sync {
    fn encode(&self, payload: &SsePayload) -> String;
    fn format_version(&self) -> u8;
}

/// v1 编码器：当前自定义 SSE 协议格式。
pub struct V1Encoder;

impl SseEncoder for V1Encoder {
    fn encode(&self, payload: &SsePayload) -> String {
        // 委托到当前 encode_message 实现
        super::protocol::encode_message_v1(payload)
    }
    fn format_version(&self) -> u8 {
        1
    }
}

/// 全局默认编码器（v2 AG-UI）。
pub fn default_encoder() -> Arc<dyn SseEncoder> {
    Arc::new(super::encoder_v2::V2Encoder)
}

/// 根据客户端声明的 SSE 协议版本解析编码器。
///
/// - `Some(2)` → `V2Encoder`（AG-UI）
/// - `None` / `Some(1)` / 其它 → `V1Encoder`
pub fn resolve_encoder(client_sse_protocol: Option<u8>) -> Arc<dyn SseEncoder> {
    match client_sse_protocol {
        Some(2) => Arc::new(super::encoder_v2::V2Encoder),
        _ => Arc::new(V1Encoder),
    }
}
