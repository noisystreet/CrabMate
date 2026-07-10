//! `structured_patch` 执行路径（`Result` 早退以降低圈复杂度）。

use std::fs;
use std::path::Path;

use serde_json::Value as JsonValue;

use crate::tools::ToolContext;
use crate::tools::file;
use crate::tools::tool_param_types::{StructuredPatchAction, StructuredPatchArgs};
use crate::workspace::changelist::record_file_state_after_write;

use super::super::{
    DataFormat, detect_format, parse_patch_query_tokens, parse_to_json, read_limited,
    remove_value_at_path, serialize_by_format, set_value_at_path,
};

pub(super) fn run_structured_patch_display(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> String {
    match run_structured_patch(args_json, working_dir, ctx) {
        Ok(s) => s,
        Err(e) => e,
    }
}

fn detect_structured_patch_format(path: &str, fmt: Option<&str>) -> Result<DataFormat, String> {
    match detect_format(path, fmt) {
        Ok(DataFormat::Csv | DataFormat::Tsv) => {
            Err("错误：structured_patch 不支持 csv/tsv，请改用 table_text 或直接编辑".to_string())
        }
        Ok(f) => Ok(f),
        Err(e) => Err(format!("错误：{}", e)),
    }
}

fn read_limited_json_value(
    abs: &Path,
    data_fmt: DataFormat,
) -> Result<(String, JsonValue), String> {
    let text = read_limited(abs).map_err(|e| e.to_string())?;
    let jv = parse_to_json(&text, data_fmt, true).map_err(|e| format!("解析失败: {}", e))?;
    Ok((text, jv))
}

fn run_structured_patch(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, String> {
    let args = parse_structured_patch_args(args_json)?;
    validate_write_intent(&args)?;
    let path = nonempty_trimmed(&args.path).ok_or_else(|| "错误：缺少 path".to_string())?;
    let query = nonempty_trimmed(args.query.trim())
        .ok_or_else(|| "错误：缺少 query（JSON Pointer 或点号路径）".to_string())?;
    let action = match args.action {
        StructuredPatchAction::Set => "set",
        StructuredPatchAction::Remove => "remove",
    };
    let fmt = args.format.map(|f| f.as_detect_token());
    let abs = file::resolve_for_read(working_dir, path).map_err(|e| format!("错误：{}", e))?;
    let data_fmt = detect_structured_patch_format(path, fmt)?;
    let (text, mut jv) = read_limited_json_value(&abs, data_fmt)?;
    let tokens = parse_patch_query_tokens(query).map_err(|e| format!("query 无效: {}", e))?;

    apply_patch_mutation(action, &args, &mut jv, tokens.as_slice())?;
    let serialized =
        serialize_by_format(&jv, data_fmt).map_err(|e| format!("序列化失败: {}", e))?;
    Ok(finish_structured_patch(StructuredPatchOutcome {
        path,
        action,
        query,
        dry_run: args.dry_run,
        abs: &abs,
        text: &text,
        serialized,
        working_dir,
        ctx,
    }))
}

fn parse_structured_patch_args(args_json: &str) -> Result<StructuredPatchArgs, String> {
    let v = crate::tools::parse_args_json(args_json)?;
    serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))
}

fn validate_write_intent(args: &StructuredPatchArgs) -> Result<(), String> {
    if !args.dry_run && !args.confirm {
        return Err(
            "错误：structured_patch 写盘需 confirm=true；建议先 dry_run=true 预览".to_string(),
        );
    }
    Ok(())
}

fn nonempty_trimmed(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

fn apply_patch_mutation(
    action: &str,
    args: &StructuredPatchArgs,
    jv: &mut JsonValue,
    tokens: &[String],
) -> Result<(), String> {
    if action == "set" {
        let new_value = args
            .value
            .clone()
            .ok_or_else(|| "错误：action=set 时必须提供 value".to_string())?;
        set_value_at_path(jv, tokens, new_value, args.create_missing)
            .map_err(|e| format!("补丁失败: {}", e))
    } else {
        remove_value_at_path(jv, tokens).map_err(|e| format!("补丁失败: {}", e))
    }
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
