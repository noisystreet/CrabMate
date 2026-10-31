// ── #1157：摘要与 Args 同源（`ToolSummaryKind::Dynamic` 的迁移目标形态）──────────
//
// 每条 `impl` 紧贴对应 Args 的 schema 定义（`tool_param_types`）阅读：字段名直接取自
// JSON，不再单独声明一份字段子集结构。于是「Args 改了、摘要没跟上」不会再以
// 「编译通过但摘要错位」的形态发生。
//
// 输出文本必须与迁移前逐字一致——`fixtures/tool_registry_golden.json` 的
// `dynamic_summary_samples` 钉住了这一点。

impl ToolArgsSummary for super::tool_param_types::ReadFileArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        let suffix = match arg_u64(args, "anchor_line") {
            Some(anchor) => format!(
                " [@L{} ±{}]",
                anchor,
                arg_u64(args, "context_lines").unwrap_or(120)
            ),
            None => read_file_range_suffix(
                arg_u64(args, "start_line"),
                arg_u64(args, "end_line"),
                arg_u64(args, "max_lines"),
            ),
        };
        let enc = arg_str(args, "encoding")
            .map(|e| format!(" enc={e}"))
            .unwrap_or_default();
        Some(format!("read file: {path}{suffix}{enc}"))
    }
}

/// `read_file` 的行区间后缀；与迁移前 `ReadFileSummaryArgs` 的分支逐字一致。
fn read_file_range_suffix(
    start: Option<u64>,
    end: Option<u64>,
    max_lines: Option<u64>,
) -> String {
    match (start, end, max_lines) {
        (Some(s), Some(e), _) => {
            let (lo, hi) = if e < s { (e, s) } else { (s, e) };
            format!(" [{lo}-{hi}]")
        }
        (Some(s), None, Some(m)) => format!(" [{s}~ max_lines={m}]"),
        (Some(s), None, None) => format!(" [{s}~]"),
        (None, Some(e), _) => format!(" [1-{e}]"),
        (None, None, Some(m)) => format!(" [chunk max_lines={m}]"),
        (None, None, None) => String::new(),
    }
}

impl ToolArgsSummary for super::tool_param_types::FileWriteArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        Some(format!("create file: {path}"))
    }
}

#[cfg(test)]
mod args_summary_tests {
    use super::*;
    use serde_json::json;

    /// 迁移后最容易回退的一处：Args 带 `deny_unknown_fields`，但摘要必须对
    /// 「多带一个字段」的 arguments 仍然生成（旧 `*SummaryArgs` 的宽容语义）。
    #[test]
    fn read_file_summary_tolerates_unknown_keys() {
        let v = json!({"path": "src/lib.rs", "bogus": 1, "anchor_line": 7});
        assert_eq!(
            super::super::tool_param_types::ReadFileArgs::summary_line(&v).as_deref(),
            Some("read file: src/lib.rs [@L7 ±120]")
        );
    }

    #[test]
    fn read_file_summary_keeps_range_and_encoding_forms() {
        let v = json!({"path": "a.rs", "start_line": 9, "end_line": 3, "encoding": " gbk "});
        assert_eq!(
            super::super::tool_param_types::ReadFileArgs::summary_line(&v).as_deref(),
            Some("read file: a.rs [3-9] enc=gbk")
        );
    }

    /// 缺 `path` 时没有摘要（与旧结构体「反序列化失败 ⇒ 无摘要」一致）。
    #[test]
    fn create_file_summary_requires_path() {
        assert!(
            super::super::tool_param_types::FileWriteArgs::summary_line(&json!({"content": ""}))
                .is_none()
        );
        assert_eq!(
            super::super::tool_param_types::FileWriteArgs::summary_line(
                &json!({"path": " notes/a.md ", "content": ""})
            )
            .as_deref(),
            Some("create file: notes/a.md")
        );
    }
}
