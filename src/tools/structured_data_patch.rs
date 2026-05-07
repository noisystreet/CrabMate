//! `structured_patch` 实现（自 `structured_data.rs` 拆分以降低圈复杂度）。

use std::fs;
use std::path::Path;

use crate::tools::ToolContext;
use crate::tools::file;
use crate::tools::tool_param_types::{StructuredPatchAction, StructuredPatchArgs};
use crate::workspace::changelist::record_file_state_after_write;

use super::{
    DataFormat, detect_format, parse_patch_query_tokens, parse_to_json, read_limited,
    remove_value_at_path, serialize_by_format, set_value_at_path,
};

pub(super) fn structured_patch_execute(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: StructuredPatchArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
    };
    let path = match args.path.trim() {
        p if !p.is_empty() => p,
        _ => return "错误：缺少 path".to_string(),
    };
    let query = match args.query.trim() {
        "" => return "错误：缺少 query（JSON Pointer 或点号路径）".to_string(),
        q => q,
    };
    let action = match args.action {
        StructuredPatchAction::Set => "set",
        StructuredPatchAction::Remove => "remove",
    };
    if !args.dry_run && !args.confirm {
        return "错误：structured_patch 写盘需 confirm=true；建议先 dry_run=true 预览".to_string();
    }
    let fmt = args.format.map(|f| f.as_detect_token());

    let abs = match file::resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e),
    };
    let data_fmt = match detect_format(path, fmt) {
        Ok(DataFormat::Csv | DataFormat::Tsv) => {
            return "错误：structured_patch 不支持 csv/tsv，请改用 table_text 或直接编辑"
                .to_string();
        }
        Ok(f) => f,
        Err(e) => return format!("错误：{}", e),
    };
    let text = match read_limited(&abs) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };
    let mut jv = match parse_to_json(&text, data_fmt, true) {
        Ok(j) => j,
        Err(e) => return format!("解析失败: {}", e),
    };
    let tokens = match parse_patch_query_tokens(query) {
        Ok(t) => t,
        Err(e) => return format!("query 无效: {}", e),
    };

    let apply_result = if action == "set" {
        let Some(new_value) = args.value.clone() else {
            return "错误：action=set 时必须提供 value".to_string();
        };
        set_value_at_path(&mut jv, &tokens, new_value, args.create_missing)
    } else {
        remove_value_at_path(&mut jv, &tokens)
    };
    if let Err(e) = apply_result {
        return format!("补丁失败: {}", e);
    }
    let serialized = match serialize_by_format(&jv, data_fmt) {
        Ok(s) => s,
        Err(e) => return format!("序列化失败: {}", e),
    };
    finish_structured_patch(StructuredPatchOutcome {
        path,
        action,
        query,
        dry_run: args.dry_run,
        abs: &abs,
        text: &text,
        serialized,
        working_dir,
        ctx,
    })
}

struct StructuredPatchOutcome<'a, 'b> {
    path: &'a str,
    action: &'a str,
    query: &'a str,
    dry_run: bool,
    abs: &'a Path,
    text: &'a str,
    serialized: String,
    working_dir: &'a Path,
    ctx: &'b ToolContext<'b>,
}

fn finish_structured_patch(o: StructuredPatchOutcome<'_, '_>) -> String {
    let qdisp = if o.query.is_empty() {
        "(root)"
    } else {
        o.query
    };
    if o.dry_run {
        return format!(
            "structured_patch 预览成功（未写入）: path={} action={} query={}\n新文件大小: {} 字节",
            o.path,
            o.action,
            qdisp,
            o.serialized.len()
        );
    }
    let before = o.text.to_owned();
    if let Err(e) = fs::write(o.abs, o.serialized.as_bytes()) {
        return format!("写入失败: {}", e);
    }
    record_file_state_after_write(
        o.ctx.workspace_changelist,
        o.working_dir,
        o.path,
        Some(before),
    );
    format!(
        "structured_patch 已写入: path={} action={} query={}",
        o.path, o.action, qdisp
    )
}
