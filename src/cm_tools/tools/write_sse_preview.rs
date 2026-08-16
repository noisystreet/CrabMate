//! Web SSE：`tool_result` 首行 `crabmate_tool_output` + **`structured_preview`** 共用的写入 diff 预览（写后展示，**不**暂停审批）。
//!
//! 体积须受限，避免阻塞 SSE 与增大会话存储。

use similar::TextDiff;

use crate::cm_tools::tool_result::{
    WorkspaceWriteDiffFields, WorkspaceWriteDiffFile, prepend_crabmate_tool_output,
};

/// 与 [`crate::cm_tools::workspace::changelist`] 单文件块上限同量级；**`structured_preview`** 中 **`git diff`**
/// 等单段 stdout 预览与写工具共用该上限。
pub const MAX_UNIFIED_PREVIEW_FILE_CHARS: usize = 6000;

/// 单次工具结果内所有文件 diff 文本总预算（字符数，近似 UTF-8 标量）。
pub const WORKSPACE_WRITE_DIFF_BUDGET_CHARS: usize = 24 * 1024;

#[derive(Debug, Clone)]
pub struct WriteDiffFileState {
    pub rel_path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

fn one_unified_diff(rel_path: &str, before: Option<&str>, after: Option<&str>) -> (String, bool) {
    let left = before.unwrap_or("");
    let right = after.unwrap_or("");
    let diff = TextDiff::from_lines(left, right);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{rel_path}"), &format!("b/{rel_path}"))
        .to_string();
    let n = unified.chars().count();
    if n <= MAX_UNIFIED_PREVIEW_FILE_CHARS {
        return (unified, false);
    }
    let truncated: String = unified
        .chars()
        .take(MAX_UNIFIED_PREVIEW_FILE_CHARS)
        .collect();
    (format!("{truncated}…\n（该文件 diff 已截断）"), true)
}

/// 在工具正文前插入**单行** JSON，供 [`crate::cm_tools::tools::structured_preview`] 与 Web 展示解析。
pub fn format_tool_output_with_write_diff_preview(
    tool_name: &str,
    body: String,
    files: Vec<WriteDiffFileState>,
    budget_chars: usize,
) -> String {
    if files.is_empty() {
        return body;
    }

    let mut remaining = budget_chars;
    let mut out_files: Vec<WorkspaceWriteDiffFile> = Vec::new();
    let mut preview_truncated = false;

    for f in files {
        let rel = f.rel_path.trim();
        if rel.is_empty() {
            continue;
        }
        if remaining < 80 {
            preview_truncated = true;
            break;
        }
        let (udiff, trunc_file) = one_unified_diff(rel, f.before.as_deref(), f.after.as_deref());
        let cost = udiff.chars().count();
        if cost > remaining {
            preview_truncated = true;
            let take = remaining.saturating_sub(80);
            let partial: String = udiff.chars().take(take).collect();
            out_files.push(WorkspaceWriteDiffFile {
                path: rel.to_string(),
                unified_diff: format!("{partial}…\n（已达本轮预览体积上限）"),
                truncated: true,
            });
            break;
        }
        remaining = remaining.saturating_sub(cost);
        out_files.push(WorkspaceWriteDiffFile {
            path: rel.to_string(),
            unified_diff: udiff,
            truncated: trunc_file,
        });
    }

    if out_files.is_empty() {
        return body;
    }

    prepend_crabmate_tool_output(
        tool_name,
        WorkspaceWriteDiffFields::new(out_files, preview_truncated),
        &body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn prepends_header_and_diff() {
        let s = format_tool_output_with_write_diff_preview(
            "create_file",
            "路径：a\nok".to_string(),
            vec![WriteDiffFileState {
                rel_path: "a.rs".to_string(),
                before: None,
                after: Some("x\n".to_string()),
            }],
            WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
        );
        let first = s.lines().next().unwrap();
        let v: Value = serde_json::from_str(first).unwrap();
        assert_eq!(v["preview"], "workspace_write_diff");
        assert_eq!(v["tool"], "create_file");
        assert!(s.contains("路径：a"));
        assert!(first.contains("crabmate_tool_output"));
    }
}
