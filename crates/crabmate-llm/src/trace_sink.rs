//! per-step trace 事件落盘：bench 与 e2e 共用的 trace 基础设施。
//!
//! `TraceSink` 为 `Option` 注入，`None` 时零开销；`run_agent_turn` / `run_batch` / e2e 测试
//! 可传入 [`FileTraceSink`] 把每次 LLM 请求、工具调用、错误落盘为 JSONL，便于事后排查。

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::Mutex;

/// 简化的 token 用量快照（与 `crabmate_types::Usage` 解耦，仅保留 trace 所需字段）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct TraceUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// 单条 trace 事件（JSONL 一行）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEvent {
    /// LLM 请求发出前
    LlmRequest {
        round: usize,
        model: String,
        messages_count: usize,
        tools_count: usize,
        fingerprint: String,
    },
    /// LLM 响应到达后
    LlmResponse {
        round: usize,
        finish_reason: String,
        /// time to first token（首字节延迟），单位毫秒；未知则 `None`
        ttft_ms: Option<u32>,
        /// 本次 LLM 调用总耗时（毫秒）
        total_ms: u64,
        usage: Option<TraceUsage>,
        /// `reasoning_content` 字符数
        reasoning_chars: usize,
        /// `content` 字符数
        content_chars: usize,
        /// 本次响应中的 `tool_calls` 数量
        tool_calls_count: usize,
    },
    /// 工具调用发起
    ToolCall {
        round: usize,
        name: String,
        /// 参数前 200 字符预览（脱敏用，避免完整入参）
        args_preview: String,
        /// 参数 SHA-256 短哈希（用于唯一标识，不泄露内容）
        args_hash: String,
    },
    /// 工具调用返回
    ToolResult {
        round: usize,
        ok: bool,
        duration_ms: u64,
        output_chars: usize,
        error_kind: Option<String>,
    },
    /// 错误事件（LLM 调用 / 工具执行 / 编排）
    Error {
        round: usize,
        kind: String,
        message: String,
    },
}

/// trace 事件 sink trait。
///
/// 实现需保证 `emit` 不阻塞调用方（内部可异步写文件或发到 channel）；
/// 且对失败容忍——sink 自身错误不应影响 agent 主流程。
#[async_trait]
pub trait TraceSink: Send + Sync {
    async fn emit(&self, event: TraceEvent);
}

/// 文件 sink：JSONL 追加写。
///
/// 每个事件序列化为一行 JSON 后追加写入；文件在构造时创建，目录自动递归创建。
pub struct FileTraceSink {
    file: Arc<Mutex<std::fs::File>>,
}

impl FileTraceSink {
    /// 在 `dir` 下创建 `<test_name>_trace.jsonl`；目录不存在则递归创建。
    pub fn create(dir: &Path, test_name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{test_name}_trace.jsonl"));
        let file = std::fs::File::create(&path)?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }
}

#[async_trait]
impl TraceSink for FileTraceSink {
    async fn emit(&self, event: TraceEvent) {
        // 序列化失败说明 TraceEvent 结构有问题，不应发生；静默丢弃避免影响主流程
        let Ok(line) = serde_json::to_string(&event) else {
            return;
        };
        let mut f = self.file.lock().await;
        use std::io::Write;
        // 写入失败（磁盘满等）静默丢弃；trace 不应阻塞 agent
        let _ = writeln!(f, "{line}");
    }
}

/// 空实现（`None` 的显式替代，便于需要 `&dyn TraceSink` 的场景）。
pub struct NullTraceSink;

#[async_trait]
impl TraceSink for NullTraceSink {
    async fn emit(&self, _event: TraceEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[tokio::test]
    async fn file_trace_sink_writes_jsonl_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let sink = FileTraceSink::create(tmp.path(), "unit").unwrap();

        sink.emit(TraceEvent::LlmRequest {
            round: 0,
            model: "test-model".into(),
            messages_count: 2,
            tools_count: 0,
            fingerprint: "abc123".into(),
        })
        .await;

        sink.emit(TraceEvent::LlmResponse {
            round: 0,
            finish_reason: "stop".into(),
            ttft_ms: Some(120),
            total_ms: 800,
            usage: Some(TraceUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                cached_tokens: Some(2),
                reasoning_tokens: None,
            }),
            reasoning_chars: 0,
            content_chars: 42,
            tool_calls_count: 0,
        })
        .await;

        // flush 后读取校验
        {
            let mut f = sink.file.lock().await;
            let _ = f.flush();
        }

        let mut content = String::new();
        let mut file = std::fs::File::open(tmp.path().join("unit_trace.jsonl")).unwrap();
        file.read_to_string(&mut content).unwrap();

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "应有 2 行 JSONL");
        assert!(lines[0].contains("\"type\":\"llm_request\""));
        assert!(lines[0].contains("\"round\":0"));
        assert!(lines[1].contains("\"type\":\"llm_response\""));
        assert!(lines[1].contains("\"finish_reason\":\"stop\""));
        assert!(lines[1].contains("\"prompt_tokens\":10"));
    }

    #[tokio::test]
    async fn null_trace_sink_silent() {
        let sink = NullTraceSink;
        // 不应 panic、不应写任何文件
        sink.emit(TraceEvent::Error {
            round: 0,
            kind: "test".into(),
            message: "should be dropped".into(),
        })
        .await;
    }
}
