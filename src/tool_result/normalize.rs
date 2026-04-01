//! `crabmate_tool` 信封与工具输出的**单一 normalize 入口**：SSE、`role: tool` 写入、CLI 展示共用同一套字段语义。
//!
//! - **载荷版本** `crabmate_tool.v`（当前仅 **1**）：与整条 SSE 控制面的 **`SseMessage.v`**（`SSE_PROTOCOL_VERSION`）不同；SSE `tool_result` 另带 **`result_version`** 与之对齐。
//! - 未知 `v`：仍尝试读取 v1 同名字段；未来 bump 时可在此集中做迁移。

use serde_json::{Map, Value};

use super::{
    ParsedLegacyOutput, ToolEnvelopeContext, parse_legacy_output, tool_error_retryable_heuristic,
};

/// 当前实现的 `crabmate_tool.v` 值；与 `encode_tool_message_envelope_v1` 写入一致。
pub const CRABMATE_TOOL_ENVELOPE_VERSION_V1: u32 = 1;

/// 归一化后的工具结果（信封 v1 形状的逻辑视图）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedToolEnvelope {
    pub envelope_version: u32,
    pub name: String,
    pub summary: String,
    pub output: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub retryable: Option<bool>,
    pub tool_call_id: Option<String>,
    pub execution_mode: Option<String>,
    pub parallel_batch_id: Option<String>,
    pub output_truncated: bool,
    pub output_original_chars: Option<u64>,
    pub output_kept_head_chars: Option<u64>,
    pub output_kept_tail_chars: Option<u64>,
}

impl NormalizedToolEnvelope {
    /// 由一次工具执行产物构造（与历史 `encode_tool_message_envelope_v1` 输入一致）。
    pub fn from_tool_run(
        tool_name: &str,
        summary: String,
        parsed: &ParsedLegacyOutput,
        raw_output: &str,
        envelope_ctx: Option<&ToolEnvelopeContext<'_>>,
    ) -> Self {
        let retryable = if parsed.ok {
            None
        } else {
            Some(tool_error_retryable_heuristic(parsed.error_code.as_deref()))
        };
        let (tool_call_id, execution_mode, parallel_batch_id) = match envelope_ctx {
            Some(c) => (
                Some(c.tool_call_id.to_string()),
                Some(c.execution_mode.to_string()),
                c.parallel_batch_id.map(|s| s.to_string()),
            ),
            None => (None, None, None),
        };
        Self {
            envelope_version: CRABMATE_TOOL_ENVELOPE_VERSION_V1,
            name: tool_name.to_string(),
            summary,
            output: raw_output.to_string(),
            ok: parsed.ok,
            exit_code: parsed.exit_code,
            error_code: parsed.error_code.clone(),
            retryable,
            tool_call_id,
            execution_mode,
            parallel_batch_id,
            output_truncated: false,
            output_original_chars: None,
            output_kept_head_chars: None,
            output_kept_tail_chars: None,
        }
    }

    fn from_crabmate_object(ct: &Map<String, Value>) -> Option<Self> {
        let envelope_version = ct
            .get("v")
            .and_then(|x| x.as_u64())
            .map(|u| u as u32)
            .unwrap_or(CRABMATE_TOOL_ENVELOPE_VERSION_V1);
        let name = ct.get("name")?.as_str()?.to_string();
        let summary = ct
            .get("summary")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let output = ct.get("output")?.as_str()?.to_string();
        let ok = ct
            .get("ok")
            .and_then(|x| x.as_bool())
            .unwrap_or_else(|| parse_legacy_output(name.as_str(), output.as_str()).ok);
        let exit_code = ct
            .get("exit_code")
            .and_then(|x| x.as_i64())
            .map(|i| i as i32);
        let error_code = ct
            .get("error_code")
            .and_then(|x| x.as_str())
            .map(String::from);
        let retryable = ct.get("retryable").and_then(|x| x.as_bool());
        let tool_call_id = ct
            .get("tool_call_id")
            .and_then(|x| x.as_str())
            .map(String::from);
        let execution_mode = ct
            .get("execution_mode")
            .and_then(|x| x.as_str())
            .map(String::from);
        let parallel_batch_id = ct
            .get("parallel_batch_id")
            .and_then(|x| x.as_str())
            .map(String::from);
        let output_truncated = ct.get("output_truncated").and_then(|x| x.as_bool()) == Some(true);
        let output_original_chars = ct.get("output_original_chars").and_then(|x| x.as_u64());
        let output_kept_head_chars = ct.get("output_kept_head_chars").and_then(|x| x.as_u64());
        let output_kept_tail_chars = ct.get("output_kept_tail_chars").and_then(|x| x.as_u64());
        Some(Self {
            envelope_version,
            name,
            summary,
            output,
            ok,
            exit_code,
            error_code,
            retryable,
            tool_call_id,
            execution_mode,
            parallel_batch_id,
            output_truncated,
            output_original_chars,
            output_kept_head_chars,
            output_kept_tail_chars,
        })
    }

    /// 若 `content` 为 `{"crabmate_tool":{...}}` 则解析为归一化视图；否则 `None`（调用方走 legacy）。
    pub fn parse_tool_message_content(content: &str) -> Option<Self> {
        let t = content.trim();
        let v: Value = serde_json::from_str(t).ok()?;
        let ct = v.get("crabmate_tool")?.as_object()?;
        Self::from_crabmate_object(ct)
    }

    pub fn to_crabmate_tool_map(&self) -> Map<String, Value> {
        let mut ct = Map::new();
        ct.insert("v".into(), Value::from(self.envelope_version));
        ct.insert("name".into(), Value::String(self.name.clone()));
        ct.insert("summary".into(), Value::String(self.summary.clone()));
        ct.insert("ok".into(), Value::Bool(self.ok));
        ct.insert("output".into(), Value::String(self.output.clone()));
        if let Some(c) = self.exit_code {
            ct.insert("exit_code".into(), Value::from(c));
        }
        if let Some(ref e) = self.error_code {
            ct.insert("error_code".into(), Value::String(e.clone()));
        }
        if let Some(r) = self.retryable {
            ct.insert("retryable".into(), Value::Bool(r));
        }
        if let Some(ref id) = self.tool_call_id {
            ct.insert("tool_call_id".into(), Value::String(id.clone()));
        }
        if let Some(ref m) = self.execution_mode {
            ct.insert("execution_mode".into(), Value::String(m.clone()));
        }
        if let Some(ref b) = self.parallel_batch_id {
            ct.insert("parallel_batch_id".into(), Value::String(b.clone()));
        }
        if self.output_truncated {
            ct.insert("output_truncated".into(), Value::Bool(true));
        }
        if let Some(n) = self.output_original_chars {
            ct.insert("output_original_chars".into(), Value::from(n));
        }
        if let Some(n) = self.output_kept_head_chars {
            ct.insert("output_kept_head_chars".into(), Value::from(n));
        }
        if let Some(n) = self.output_kept_tail_chars {
            ct.insert("output_kept_tail_chars".into(), Value::from(n));
        }
        ct
    }

    pub fn encode_to_message_line(&self) -> String {
        let mut root = Map::new();
        root.insert(
            "crabmate_tool".into(),
            Value::Object(self.to_crabmate_tool_map()),
        );
        serde_json::to_string(&Value::Object(root)).unwrap_or_else(|_| self.output.clone())
    }
}

/// 若 `content` 为 `{"crabmate_tool":{...}}` 则解析为归一化视图；否则 `None`。
pub fn normalize_tool_message_content(content: &str) -> Option<NormalizedToolEnvelope> {
    NormalizedToolEnvelope::parse_tool_message_content(content)
}
