//! SSE 解析器 trait：将 `data:` 行文本解析为控制面事件并分发到回调。
//!
//! 当前 v1 为当前协议格式；后续 v2（AG-UI）新增 `V2Parser` 实现同一 trait。

use crate::sse_dispatch::{SseControlSink, SseDispatch, try_dispatch_sse_control_payload};

/// SSE 解析器：从 `data:` 行文本解析控制面事件并分发。
pub(crate) trait SseParser {
    /// 解析 `data` 并分发给 `sink` 中的回调。
    fn parse(&self, data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch;
    /// 返回协议版本号（Phase 2 中用于选择解析器）。
    #[allow(dead_code)]
    fn protocol_version(&self) -> u8;
}

/// v1 解析器：当前自定义 SSE 协议的解析逻辑。
pub(crate) struct V1Parser;

impl SseParser for V1Parser {
    fn parse(&self, data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch {
        try_dispatch_sse_control_payload(data, sink)
    }
    fn protocol_version(&self) -> u8 {
        1
    }
}

pub(crate) use super::parser_v2::V2Parser;
