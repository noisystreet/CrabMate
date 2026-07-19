//! SSE 事件流解析器：从 HTTP 响应体中逐事件解析 `event: xxx\ndata: yyy\n\n` 格式。
//!
//! # 使用示例
//!
//! ```ignore
//! let resp = client.post(url).body(body).send().await.unwrap();
//! let mut stream = SseEventStream::new(resp);
//! while let Some(ev) = stream.next_event().await {
//!     match ev.event.as_str() {
//!         "content_delta" => { /* 追加内容 */ }
//!         "done" => break,
//!         "error" => panic!("SSE 错误: {}", ev.data),
//!         _ => {}
//!     }
//! }
//! ```

/// 单个 SSE 事件。
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// SSE 流解析器：包装 `reqwest::Response` 的字节流，按 `\n\n` 分帧。
///
/// 内部缓冲处理跨帧边界。
pub struct SseEventStream {
    inner: reqwest::Response,
    buf: Vec<u8>,
}

impl SseEventStream {
    pub fn new(resp: reqwest::Response) -> Self {
        Self {
            inner: resp,
            buf: Vec::with_capacity(4096),
        }
    }

    /// 读取下一个 SSE 事件。
    ///
    /// 返回 `None` 表示流已结束（连接关闭）。
    pub async fn next_event(&mut self) -> Option<SseEvent> {
        loop {
            // 尝试从当前缓冲区解析一个完整事件
            if let Some(event) = Self::parse_one(&mut self.buf) {
                return Some(event);
            }

            // 读取更多数据
            let chunk = self.inner.chunk().await.ok()??;
            if chunk.is_empty() {
                // 流结束：缓冲区剩余内容作为尾部数据
                if !self.buf.is_empty() {
                    let data = String::from_utf8_lossy(&self.buf).to_string();
                    self.buf.clear();
                    return Some(SseEvent {
                        event: String::new(),
                        data,
                    });
                }
                return None;
            }
            self.buf.extend_from_slice(&chunk);
        }
    }

    /// 从缓冲区解析一个 SSE 事件（`event: xxx\ndata: yyy\n\n`）。
    /// 返回 `None` 表示缓冲区中尚无完整事件。
    /// 空帧（纯 `\n\n` 心跳）也返回 `None`，由上层重试。
    fn parse_one(buf: &mut Vec<u8>) -> Option<SseEvent> {
        // 查找双换行分隔符
        let s = std::str::from_utf8(buf).ok()?;
        let double_nl = s.find("\n\n")?;

        let frame_end = double_nl + 2; // 包含 "\n\n"
        let frame = s[..frame_end].to_string();

        // 消耗缓冲区中的已解析帧
        buf.drain(..frame_end);

        let trimmed = frame.trim();
        // 空帧（心跳）或纯注释行，跳过
        if trimmed.is_empty() || trimmed == ":" || trimmed.starts_with(':') {
            return None;
        }

        let mut event = String::new();
        let mut data = String::new();

        for line in frame.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("data:") {
                data = value.trim().to_string();
            }
            // 忽略其他字段（id, retry 等）
        }

        Some(SseEvent { event, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let mut buf = b"event: content_delta\ndata: Hello, world!\n\n".to_vec();
        let ev = SseEventStream::parse_one(&mut buf).unwrap();
        assert_eq!(ev.event, "content_delta");
        assert_eq!(ev.data, "Hello, world!");
        assert!(buf.is_empty());
    }

    #[test]
    fn parse_multiple_events() {
        let input = b"event: a\ndata: 1\n\nevent: b\ndata: 2\n\n";
        let mut buf = input.to_vec();

        let ev1 = SseEventStream::parse_one(&mut buf).unwrap();
        assert_eq!(ev1.event, "a");
        assert_eq!(ev1.data, "1");

        let ev2 = SseEventStream::parse_one(&mut buf).unwrap();
        assert_eq!(ev2.event, "b");
        assert_eq!(ev2.data, "2");

        assert!(buf.is_empty());
    }

    #[test]
    fn partial_frame_returns_none() {
        let mut buf = b"event: a\ndata: 1\n".to_vec(); // 缺少 \n\n
        assert!(SseEventStream::parse_one(&mut buf).is_none());
        // 缓冲区不变
        assert_eq!(buf, b"event: a\ndata: 1\n");
    }

    #[test]
    fn data_only_event() {
        let mut buf = b"data: {\"key\":\"value\"}\n\n".to_vec();
        let ev = SseEventStream::parse_one(&mut buf).unwrap();
        assert_eq!(ev.event, "");
        assert_eq!(ev.data, r#"{"key":"value"}"#);
    }
}
