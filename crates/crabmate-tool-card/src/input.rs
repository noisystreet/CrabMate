//! 工具卡展示输入（与 SSE `tool_result` / `crabmate_tool` 信封字段对齐）。

use serde_json::Value;

/// 与 Web SSE [`ToolResultInfo`]、持久化 `crabmate_tool` 信封同形的展示输入。
#[derive(Debug, Clone)]
pub struct ToolCardInput {
    pub name: String,
    pub goal_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub result_version: u32,
    pub summary: Option<String>,
    pub output: String,
    pub ok: Option<bool>,
    pub exit_code: Option<i64>,
    pub error_code: Option<String>,
    pub failure_category: Option<String>,
    pub structured_preview: Option<Value>,
}

/// 主仓 `tool_result::NormalizedToolEnvelope` 同名字段（避免 `from_*` 超长参数列表）。
#[derive(Debug, Clone)]
pub struct NormalizedToolSnapshotFields {
    pub name: String,
    pub summary: String,
    pub output: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub failure_category: Option<String>,
    pub tool_call_id: Option<String>,
    pub structured_payload: Option<Value>,
}

impl ToolCardInput {
    /// 由后端归一化信封字段构造。
    #[must_use]
    pub fn from_normalized_fields(fields: NormalizedToolSnapshotFields) -> Self {
        let NormalizedToolSnapshotFields {
            name,
            summary,
            output,
            ok,
            exit_code,
            error_code,
            failure_category,
            tool_call_id,
            structured_payload,
        } = fields;
        Self {
            name,
            goal_id: None,
            tool_call_id,
            result_version: 1,
            summary: if summary.trim().is_empty() {
                None
            } else {
                Some(summary)
            },
            output,
            ok: Some(ok),
            exit_code: exit_code.map(i64::from),
            error_code,
            failure_category,
            structured_preview: structured_payload,
        }
    }
}
