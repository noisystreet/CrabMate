//! `structured_patch` 执行路径（`Result` 早退以降低圈复杂度）。

use std::path::Path;

use serde_json::Value as JsonValue;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::file;
use crate::cm_tools::tools::tool_param_types::{StructuredPatchAction, StructuredPatchArgs};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;

use super::super::{
    DataFormat, detect_format, parse_patch_query_tokens, parse_to_json, read_limited,
    remove_value_at_path, serialize_by_format, set_value_at_path, structured_io_error,
};

/// 执行 `structured_patch`：参数 / 查询 / revision 问题为 `invalid_args`，
/// 读盘为 `structured_patch_io_failed`，解析为 `structured_patch_parse_failed`，
/// 写盘为 `structured_patch_write_failed`。
#[allow(clippy::result_large_err)]
pub(super) fn run_structured_patch_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    run_structured_patch(args_json, working_dir, ctx)
}

#[allow(clippy::result_large_err)]
fn detect_structured_patch_format(path: &str, fmt: Option<&str>) -> Result<DataFormat, ToolError> {
    match detect_format(path, fmt) {
        Ok(DataFormat::Csv | DataFormat::Tsv) => Err(ToolError::invalid_args(
            "错误：structured_patch 不支持 csv/tsv，请改用 table_text 或直接编辑".to_string(),
        )),
        Ok(f) => Ok(f),
        Err(e) => Err(ToolError::invalid_args(format!("错误：{e}"))),
    }
}

/// 读盘错误经 [`structured_io_error`] 分流（「文件过大」→ `invalid_args`，其余 → IO）。
/// 迁移前这里直接透传 `e.to_string()`，首行无关键词 ⇒ 会被判成成功。
#[allow(clippy::result_large_err)]
fn read_limited_json_value(
    abs: &Path,
    data_fmt: DataFormat,
) -> Result<(String, JsonValue), ToolError> {
    let text = read_limited(abs).map_err(structured_io_error)?;
    let jv = parse_to_json(&text, data_fmt, true).map_err(|e| {
        ToolError::external_code("structured_patch_parse_failed", format!("解析失败: {e}"))
    })?;
    Ok((text, jv))
}

#[allow(clippy::result_large_err)]
fn run_structured_patch(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let raw_args =
        crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let (args, path, query, action) = prepare_structured_patch(&raw_args)?;
    let fmt = args.format.map(|f| f.as_detect_token());
    // 读写共用 `resolve_for_read`：它 `canonicalize()` 后返回**真实路径**，正是原子替换（rename）
    // 需要的目标形态——若换成 `resolve_for_write`（保留符号链接前缀），rename 会把链接本身
    // 替换成普通文件。同时它要求目标已存在，而 patch 本来就必须先读原文，语义吻合。
    let abs = file::resolve_for_read(working_dir, &path).map_err(ToolError::path_resolve_failed)?;
    let data_fmt = detect_structured_patch_format(&path, fmt)?;
    let (text, mut jv) = read_limited_json_value(&abs, data_fmt)?;
    let tokens =
        parse_patch_query_tokens(&query).map_err(|e| ToolError::invalid_args(format!("query 无效: {e}")))?;

    // 显式 `base_revision` 在**预览阶段也校验**（与 `modify_file` 一致）：预览若基于过期快照，
    // 应当立刻报错，而不是给出一份「看起来安全」的 diff 让人据此决定写盘。
    // 写盘路径会在 `write_bytes_with_revision_guard` 里紧贴 rename 前再校验一次——两次不是
    // 重复劳动：后者只覆盖「预览 → rename」之间目标被改写的 TOCTOU 窗口。
    file::verify_write_revision(&raw_args, &abs, ctx, false).map_err(ToolError::invalid_args)?;

    apply_patch_mutation(action, &args, &mut jv, tokens.as_slice())?;
    let serialized = serialize_by_format(&jv, data_fmt).map_err(|e| {
        ToolError::external_code("structured_patch_serialize_failed", format!("序列化失败: {e}"))
    })?;
    finish_structured_patch(StructuredPatchOutcome {
        path: &path,
        action,
        query: &query,
        dry_run: args.dry_run,
        abs: &abs,
        text: &text,
        serialized,
        raw_args: &raw_args,
        working_dir,
        ctx,
    })
}

/// 解析并校验结构化补丁参数，返回规范化后的 `(args, path, query, action 文案)`。
#[allow(clippy::result_large_err)]
fn prepare_structured_patch(
    raw: &JsonValue,
) -> Result<(StructuredPatchArgs, String, String, &'static str), ToolError> {
    let args = parse_structured_patch_args(raw)?;
    let path = nonempty_trimmed(&args.path)
        .ok_or_else(|| ToolError::invalid_args("错误：缺少 path".to_string()))?
        .to_string();
    let query = nonempty_trimmed(args.query.trim())
        .ok_or_else(|| {
            ToolError::invalid_args("错误：缺少 query（JSON Pointer 或点号路径）".to_string())
        })?
        .to_string();
    let action = match args.action {
        StructuredPatchAction::Set => "set",
        StructuredPatchAction::Remove => "remove",
    };
    Ok((args, path, query, action))
}

#[allow(clippy::result_large_err)]
fn parse_structured_patch_args(raw: &JsonValue) -> Result<StructuredPatchArgs, ToolError> {
    crate::cm_tools::tools::parse_args_from_value(raw.clone()).map_err(ToolError::invalid_args)
}

fn nonempty_trimmed(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

#[allow(clippy::result_large_err)]
fn apply_patch_mutation(
    action: &str,
    args: &StructuredPatchArgs,
    jv: &mut JsonValue,
    tokens: &[String],
) -> Result<(), ToolError> {
    if action == "set" {
        let new_value = args
            .value
            .clone()
            .ok_or_else(|| ToolError::invalid_args("错误：action=set 时必须提供 value".to_string()))?;
        set_value_at_path(jv, tokens, new_value, args.create_missing)
            .map_err(|e| ToolError::invalid_args(format!("补丁失败: {e}")))
    } else {
        remove_value_at_path(jv, tokens).map_err(|e| ToolError::invalid_args(format!("补丁失败: {e}")))
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
    /// 原始入参 JSON：`base_revision` / `allow_stale_revision` 由写侧守卫直接读取。
    raw_args: &'a JsonValue,
    working_dir: &'a Path,
    ctx: &'b ToolContext<'b>,
}

/// 预览/写入共用的 unified diff（同 `text_diff` 渲染，带字节上限）。
fn render_patch_diff(path: &str, before: &str, after: &str) -> String {
    let diff = crate::cm_tools::tools::text_diff::diff_unified(
        before,
        after,
        &format!("a/{path}"),
        &format!("b/{path}"),
        3,
    );
    crate::cm_tools::tools::output_util::truncate_output_bytes(
        &diff,
        crate::cm_tools::tools::write_sse_preview::WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

#[allow(clippy::result_large_err)]
fn finish_structured_patch(o: StructuredPatchOutcome<'_, '_>) -> Result<String, ToolError> {
    let qdisp = if o.query.is_empty() {
        "(root)"
    } else {
        o.query
    };
    if o.dry_run {
        // diff 只在这里生成：写盘路径不需要它，别为一次覆盖算整份文本 diff。
        let diff = render_patch_diff(o.path, o.text, &o.serialized);
        let body = if diff.trim().is_empty() {
            "（内容无变化；`dry_run=false` 仍会原子替换文件，仅 inode/修改时间变化）".to_string()
        } else {
            diff
        };
        return Ok(format!(
            "structured_patch 预览成功（未写入）: path={} action={} query={}\n{}",
            o.path, o.action, qdisp, body
        ));
    }
    let before = o.text.to_owned();
    // 统一提交入口：tmp → 继承元数据 → fsync → revision 校验 → 原子替换。
    // `auto_bind=false`：键路径编辑不受行号漂移影响，只校验调用方显式传入的 `base_revision`。
    if let Err(e) = file::write_bytes_with_revision_guard(
        o.abs,
        o.serialized.as_bytes(),
        o.raw_args,
        o.ctx,
        false,
    ) {
        return Err(ToolError::external_code("structured_patch_write_failed", e));
    }
    record_file_state_after_write(
        o.ctx.workspace_changelist,
        o.working_dir,
        o.path,
        Some(before),
    );
    Ok(format!(
        "structured_patch 已写入: path={} action={} query={}",
        o.path, o.action, qdisp
    ))
}
