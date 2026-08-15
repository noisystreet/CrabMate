//! 工具正文首行 **`crabmate_tool_output`** 头：serde 真源，避免各工具手写 `json!` 漂移。
//!
//! JSON 形状保持扁平（`kind` / `tool` / `version` + flatten 字段），与既有 Web / 卡片解析兼容。
//! 错误路径不带该头；写预览（[`PREVIEW_WORKSPACE_WRITE_DIFF`]）可与失败正文并存。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 与历史 `json!` 头一致。
pub const CRABMATE_TOOL_OUTPUT_KIND: &str = "crabmate_tool_output";
/// 载荷版本；与既有 `version: 1` 头对齐。
pub const CRABMATE_TOOL_OUTPUT_VERSION: u32 = 1;
/// 写工具 / `git diff` 预览头的 `preview` 值。
pub const PREVIEW_WORKSPACE_WRITE_DIFF: &str = "workspace_write_diff";

/// 解析成败时只需这几个键；其余字段忽略。
#[derive(Debug, Clone, Deserialize)]
pub struct CrabmateToolOutputMeta {
    pub kind: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub preview: Option<String>,
}

impl CrabmateToolOutputMeta {
    pub fn parse_line(first: &str) -> Option<Self> {
        let meta: Self = serde_json::from_str(first).ok()?;
        meta.is_crabmate_tool_output().then_some(meta)
    }

    pub fn is_crabmate_tool_output(&self) -> bool {
        self.kind == CRABMATE_TOOL_OUTPUT_KIND
    }

    pub fn is_workspace_write_diff(&self) -> bool {
        self.preview.as_deref() == Some(PREVIEW_WORKSPACE_WRITE_DIFF)
    }
}

/// 扁平信封：序列化后与历史单行 JSON 同形。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CrabmateToolOutputEnvelope<T> {
    pub kind: String,
    pub tool: String,
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(flatten)]
    pub fields: T,
}

impl<T> CrabmateToolOutputEnvelope<T> {
    pub fn new(tool: impl Into<String>, fields: T) -> Self {
        Self {
            kind: CRABMATE_TOOL_OUTPUT_KIND.to_string(),
            tool: tool.into(),
            version: CRABMATE_TOOL_OUTPUT_VERSION,
            ok: None,
            fields,
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error>
    where
        T: Serialize,
    {
        serde_json::to_value(self)
    }
}

/// 在正文前插入单行头。这些类型的序列化失败视为内部不变量破坏。
pub fn prepend_crabmate_tool_output<T: Serialize>(tool: &str, fields: T, body: &str) -> String {
    let line = serde_json::to_string(&CrabmateToolOutputEnvelope::new(tool, fields))
        .unwrap_or_else(|e| panic!("crabmate_tool_output 头序列化失败（内部错误）: {e}"));
    format!("{line}\n{body}")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileOutputFields {
    pub path: String,
    pub start_line: usize,
    pub end_line_shown: usize,
    pub line_count_returned: usize,
    pub total_lines: Option<usize>,
    pub truncated_by_max_lines: bool,
    pub has_more: bool,
    pub file_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchInFilesOutputFields {
    pub pattern: String,
    pub root: String,
    pub match_count: usize,
    pub files_visited: usize,
    pub max_results: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadDirOutputFields {
    pub path: String,
    pub entries_shown: usize,
    pub entries_walked: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListTreeOutputFields {
    pub path: String,
    pub max_depth: usize,
    pub max_entries: usize,
    pub include_hidden: bool,
    pub lines_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceWriteDiffFile {
    pub path: String,
    pub unified_diff: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceWriteDiffFields {
    pub preview: String,
    pub files: Vec<WorkspaceWriteDiffFile>,
    pub preview_truncated: bool,
}

impl WorkspaceWriteDiffFields {
    pub fn new(files: Vec<WorkspaceWriteDiffFile>, preview_truncated: bool) -> Self {
        Self {
            preview: PREVIEW_WORKSPACE_WRITE_DIFF.to_string(),
            files,
            preview_truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_file_header_keeps_total_lines_null_and_omits_ok() {
        let line = serde_json::to_string(&CrabmateToolOutputEnvelope::new(
            "read_file",
            ReadFileOutputFields {
                path: "a.py".into(),
                start_line: 1,
                end_line_shown: 2,
                line_count_returned: 2,
                total_lines: None,
                truncated_by_max_lines: false,
                has_more: false,
                file_empty: false,
            },
        ))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["kind"], CRABMATE_TOOL_OUTPUT_KIND);
        assert_eq!(v["tool"], "read_file");
        assert_eq!(v["version"], 1);
        assert!(v.get("total_lines").unwrap().is_null());
        assert!(v.get("ok").is_none());
        assert_eq!(v["path"], "a.py");
    }

    #[test]
    fn write_diff_header_sets_preview_constant() {
        let fields = WorkspaceWriteDiffFields::new(
            vec![WorkspaceWriteDiffFile {
                path: "a.rs".into(),
                unified_diff: "diff".into(),
                truncated: false,
            }],
            false,
        );
        let v = CrabmateToolOutputEnvelope::new("create_file", fields)
            .to_json_value()
            .unwrap();
        assert_eq!(v["preview"], PREVIEW_WORKSPACE_WRITE_DIFF);
        assert_eq!(v["files"][0]["path"], "a.rs");
        let meta = CrabmateToolOutputMeta::parse_line(&v.to_string()).unwrap();
        assert!(meta.is_workspace_write_diff());
    }

    #[test]
    fn meta_ignores_payload_fields() {
        let raw = json!({
            "kind": CRABMATE_TOOL_OUTPUT_KIND,
            "tool": "read_file",
            "version": 1,
            "path": "x",
            "has_more": false
        })
        .to_string();
        let meta = CrabmateToolOutputMeta::parse_line(&raw).unwrap();
        assert_eq!(meta.tool.as_deref(), Some("read_file"));
        assert_eq!(meta.ok, None);
        assert!(!meta.is_workspace_write_diff());
    }
}
