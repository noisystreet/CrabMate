//! AG-UI 解析器 trait：将 `data:` 行文本解析为控制面事件并分发到回调。
//!
//! 当前仅 AG-UI（v2）协议；`V2Parser` 为唯一实现。

use crate::sse_dispatch::{SseControlSink, SseDispatch};

/// SSE 解析器：从 `data:` 行文本解析控制面事件并分发。
pub(crate) trait SseParser {
    /// 解析 `data` 并分发给 `sink` 中的回调。
    fn parse(&self, data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch;
}

pub(crate) use super::parser_v2::V2Parser;
