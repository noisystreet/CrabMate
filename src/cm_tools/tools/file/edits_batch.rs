//! `modify_file` 的批量 `edits` 模式：一次调用携带多条局部编辑，基于**同一磁盘快照**
//! 自底向上应用，行号互不偏移——避免同轮多次调用 `replace_lines` / `insert_after_line`
//! 时前一次编辑导致后续行号失效的问题。

use std::path::Path;

use serde_json::Value;

use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;
use crate::cm_tools::workspace::fs::write_bytes_under_root;

use super::path::{canonical_workspace_root, tool_user_error_from_workspace_path};
use super::replace_lines_stream::{verify_expect_content, verify_expect_line_content};

/// 单次 `edits` 批量的条数上限，防止一次调用写坏大文件。
const MAX_EDITS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Replace,
    Insert,
}

struct BatchEdit {
    kind: EditKind,
    /// replace：区间起点 start_line；insert：锚点 after_line。
    start_line: usize,
    /// replace：区间终点 end_line；insert：与 start_line 相同。
    end_line: usize,
    body: String,
    expect_content: Option<String>,
    expect_line_content: Option<String>,
    /// 错误信息中的条目描述，如 `edits[0] replace_lines 3-5`。
    desc: String,
}

impl BatchEdit {
    /// 自底向上排序键：replace 用区间起点；insert 用锚点行。
    fn position(&self) -> usize {
        self.start_line
    }

    fn is_replace(&self) -> bool {
        self.kind == EditKind::Replace
    }
}

/// 解析顶层参数：与单条局部编辑字段互斥，逐条解析 `edits[i]`。
fn parse_batch_edits(v: &Value) -> Result<Vec<BatchEdit>, String> {
    for key in ["content", "start_line", "end_line", "after_line"] {
        if v.get(key).is_some() {
            return Err(format!(
                "错误：edits 批量编辑与顶层 {key} 互斥；请把 content 与行号写进每条 edits[i]，或去掉 edits 改用单条局部编辑。"
            ));
        }
    }
    if let Some(mode) = v.get("mode").and_then(|m| m.as_str()) {
        return Err(format!(
            "错误：edits 批量编辑与顶层 mode={mode:?} 互斥；mode 请写在每条 edits[i] 内。"
        ));
    }
    if v.get("expect_content").is_some() || v.get("expect_line_content").is_some() {
        return Err(
            "错误：edits 批量编辑与顶层 expect_content/expect_line_content 互斥；守卫参数请写入对应的 edits[i] 条目内，否则会被静默忽略。"
                .to_string(),
        );
    }
    let Some(Value::Array(arr)) = v.get("edits") else {
        return Err("错误：edits 需为数组".to_string());
    };
    if arr.is_empty() {
        return Err("错误：edits 为空数组；请至少提供一条编辑".to_string());
    }
    if arr.len() > MAX_EDITS {
        return Err(format!(
            "错误：edits 一次最多 {} 条（收到 {}）；请拆分为多次调用",
            MAX_EDITS,
            arr.len()
        ));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        out.push(parse_one_edit(i, item)?);
    }
    Ok(out)
}

/// 单条编辑的 mode/类型推断：显式 mode 优先，其次按是否带行号/锚点推断。
fn parse_edit_kind(prefix: &str, item: &Value) -> Result<EditKind, String> {
    let has_line_range = item.get("start_line").is_some() || item.get("end_line").is_some();
    let has_anchor = item.get("after_line").is_some();
    match item.get("mode").and_then(|m| m.as_str()).map(str::trim) {
        None => {
            if has_line_range {
                Ok(EditKind::Replace)
            } else if has_anchor {
                Ok(EditKind::Insert)
            } else {
                Err(format!(
                    "错误：{prefix} 缺少行号（replace_lines 需 start_line/end_line；insert_after_line 需 after_line）"
                ))
            }
        }
        Some("replace_lines" | "lines" | "line" | "replace" | "replacelines" | "partial") => {
            Ok(EditKind::Replace)
        }
        Some("insert_after_line" | "insert_after" | "insert" | "insert_line"
        | "append_after_line") => Ok(EditKind::Insert),
        Some(other) => Err(format!(
            "错误：{prefix} 的 mode 仅支持 replace_lines / insert_after_line（收到 {other:?}）"
        )),
    }
}

/// 单条编辑的行区间解析：replace 取 start/end（缺 end 用 start，自动交换），
/// insert 取 after_line。
fn parse_edit_range(prefix: &str, item: &Value, kind: EditKind) -> Result<(usize, usize), String> {
    match kind {
        EditKind::Replace => {
            let s = item
                .get("start_line")
                .and_then(|n| n.as_u64())
                .filter(|n| *n >= 1)
                .ok_or_else(|| format!("错误：{prefix} 需要 start_line（>=1）"))?
                as usize;
            let e = item
                .get("end_line")
                .and_then(|n| n.as_u64())
                .filter(|n| *n >= 1)
                .map(|n| n as usize)
                .unwrap_or(s);
            Ok((s.min(e), s.max(e)))
        }
        EditKind::Insert => {
            let a = item
                .get("after_line")
                .and_then(|n| n.as_u64())
                .ok_or_else(|| format!("错误：{prefix} 需要 after_line（>=0）"))?
                as usize;
            Ok((a, a))
        }
    }
}

/// expect_* 与编辑类型的交叉校验：expect_content 只配 replace，expect_line_content 只配 insert。
fn check_edit_expect_kinds(
    prefix: &str,
    kind: EditKind,
    expect_content: Option<&String>,
    expect_line_content: Option<&String>,
) -> Result<(), String> {
    match kind {
        EditKind::Replace if expect_line_content.is_some() => Err(format!(
            "错误：{prefix}：expect_line_content 仅用于 insert_after_line 的锚点行校验；replace_lines 请使用 expect_content。"
        )),
        EditKind::Insert if expect_content.is_some() => Err(format!(
            "错误：{prefix}：expect_content 仅用于 replace_lines 的区间内容校验；insert_after_line 请使用 expect_line_content。"
        )),
        _ => Ok(()),
    }
}

fn parse_one_edit(i: usize, item: &Value) -> Result<BatchEdit, String> {
    let prefix = format!("edits[{i}]");
    let kind = parse_edit_kind(&prefix, item)?;
    let body = item
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| format!("错误：{prefix} 缺少 content"))?
        .to_string();
    let (start_line, end_line) = parse_edit_range(&prefix, item, kind)?;
    let expect_content = item
        .get("expect_content")
        .and_then(|c| c.as_str())
        .map(String::from);
    let expect_line_content = item
        .get("expect_line_content")
        .and_then(|c| c.as_str())
        .map(String::from);
    check_edit_expect_kinds(&prefix, kind, expect_content.as_ref(), expect_line_content.as_ref())?;
    let desc = match kind {
        EditKind::Replace => format!("{prefix} replace_lines {start_line}-{end_line}"),
        EditKind::Insert => format!("{prefix} insert_after_line {start_line}"),
    };
    Ok(BatchEdit {
        kind,
        start_line,
        end_line,
        body,
        expect_content,
        expect_line_content,
        desc,
    })
}

/// 冲突检测：任何两条编辑在原文件（快照）中的区间/锚点重叠即拒绝——
/// 同一快照上的重叠编辑语义不明确，自底向上也无法消除歧义。
fn check_edits_conflicts(edits: &[BatchEdit]) -> Result<(), String> {
    for i in 0..edits.len() {
        for j in (i + 1)..edits.len() {
            let (a, b) = (&edits[i], &edits[j]);
            let conflict = match (a.kind, b.kind) {
                (EditKind::Replace, EditKind::Replace) => {
                    a.start_line <= b.end_line && b.start_line <= a.end_line
                }
                // insert 锚点行落在对方替换区间内部（不含末行）时冲突；
                // 锚点等于区间末行=在区间之后插入，合法。
                (EditKind::Insert, EditKind::Replace) => {
                    b.start_line <= a.start_line && a.start_line < b.end_line
                }
                (EditKind::Replace, EditKind::Insert) => {
                    a.start_line <= b.start_line && b.start_line < a.end_line
                }
                (EditKind::Insert, EditKind::Insert) => false,
            };
            if conflict {
                return Err(format!(
                    "错误：{} 与 {} 的行区间/锚点在原文件中重叠冲突；请合并为一条编辑或拆分调用",
                    a.desc, b.desc
                ));
            }
        }
    }
    Ok(())
}

/// 越界校验（基于快照行数）：replace 的区间与 insert 的锚点都必须落在文件内。
fn check_edits_bounds(edits: &[BatchEdit], total: usize) -> Result<(), String> {
    for e in edits {
        let bad = match e.kind {
            EditKind::Replace => e.start_line > total || e.end_line > total,
            EditKind::Insert => e.start_line > total,
        };
        if bad {
            return Err(format!(
                "错误：{}：行号超出文件行数（文件共 {} 行）",
                e.desc, total
            ));
        }
    }
    Ok(())
}

/// 应用前的 expect_* 守卫：全部基于同一快照逐条校验。
fn verify_edit_expectations(view: &[&str], edits: &[BatchEdit]) -> Result<(), String> {
    for e in edits {
        match e.kind {
            EditKind::Replace => {
                if let Some(expect) = &e.expect_content
                    && let Err(err) = verify_expect_content(view, e.start_line, e.end_line, expect)
                {
                    return Err(format!("{}：{}", e.desc, err));
                }
            }
            EditKind::Insert => {
                if let Some(expect) = &e.expect_line_content
                    && let Err(err) = verify_expect_line_content(view, e.start_line, expect)
                {
                    return Err(format!("{}：{}", e.desc, err));
                }
            }
        }
    }
    Ok(())
}

/// 自底向上应用全部编辑：按位置降序排序后逐条 splice，靠前编辑不受靠后编辑影响。
fn apply_edits(lines: &mut Vec<String>, edits: &[BatchEdit]) {
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by(|&x, &y| {
        let (a, b) = (&edits[x], &edits[y]);
        // 位置降序（自底向上）；同位置时 insert 先于 replace 应用：
        // 冲突检测允许「insert 锚点 == replace 区间末行」，此时必须先插入再替换，
        // 否则多行/删除替换会把插入内容埋进替换块内部或下移一行。
        b.position()
            .cmp(&a.position())
            .then_with(|| a.is_replace().cmp(&b.is_replace()))
    });
    for idx in order {
        let e = &edits[idx];
        match e.kind {
            EditKind::Replace => {
                let new_lines: Vec<String> = if e.body.is_empty() {
                    Vec::new()
                } else {
                    e.body.lines().map(String::from).collect()
                };
                lines.splice(e.start_line - 1..e.end_line, new_lines);
            }
            EditKind::Insert => {
                if e.body.is_empty() {
                    continue;
                }
                let at = e.start_line;
                for (k, line) in e.body.lines().map(String::from).enumerate() {
                    lines.insert(at + k, line);
                }
            }
        }
    }
}

/// dry_run 预览输出：展示应用全部编辑后的完整内容 diff。
fn batch_dry_run_preview(path: &str, edits: &[BatchEdit], original: &str, out: String) -> String {
    let body = format!(
        "预览（dry_run=true）：已应用 {} 条局部编辑未写盘。设置 dry_run=false 以执行。",
        edits.len()
    );
    format_tool_output_with_write_diff_preview(
        "modify_file",
        body,
        vec![WriteDiffFileState {
            rel_path: path.to_string(),
            before: Some(original.to_string()),
            after: Some(out),
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

/// 写盘成功后的收尾：记录 changelist 并输出 before/after diff 预览。
fn finish_batch_success(
    ctx: &ToolContext<'_>,
    working_dir: &Path,
    path: &str,
    edits: &[BatchEdit],
    original: &str,
    target: &Path,
) -> String {
    let replace_n = edits.iter().filter(|e| e.is_replace()).count();
    let insert_n = edits.len() - replace_n;
    let body = format!(
        "已应用 {} 条局部编辑（replace_lines×{}、insert_after_line×{}），基于同一磁盘快照自底向上写入",
        edits.len(),
        replace_n,
        insert_n
    );
    record_file_state_after_write(
        ctx.workspace_changelist,
        working_dir,
        path,
        Some(original.to_string()),
    );
    let after = std::fs::read_to_string(target).ok();
    format_tool_output_with_write_diff_preview(
        "modify_file",
        body,
        vec![WriteDiffFileState {
            rel_path: path.to_string(),
            before: Some(original.to_string()),
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

/// 读取磁盘快照并应用全部编辑：冲突/越界/expect 守卫 → 自底向上 splice →
/// 尾换行保持原状。返回 (原文件内容, 编辑后内容)。
fn run_batch_edits_on_snapshot(
    target: &Path,
    edits: &[BatchEdit],
) -> Result<(String, String), String> {
    let original = std::fs::read_to_string(target).map_err(|e| {
        format!(
            "错误：读取原文件失败: {}（批量 edits 需可读的 UTF-8 文本文件）",
            e
        )
    })?;
    let mut lines: Vec<String> = original.lines().map(String::from).collect();

    check_edits_conflicts(edits)?;
    check_edits_bounds(edits, lines.len())?;
    let view: Vec<&str> = lines.iter().map(String::as_str).collect();
    verify_edit_expectations(&view, edits)?;

    apply_edits(&mut lines, edits);

    // 尾换行保持原状：原文件以 \n 结尾（或为空文件被插入内容）时输出保留尾换行。
    let mut out = lines.join("\n");
    if !lines.is_empty() && (original.ends_with('\n') || original.is_empty()) {
        out.push('\n');
    }
    Ok((original, out))
}

/// 批量 `edits` 入口：读一次全文 → 冲突/越界/expect 守卫 → 自底向上应用 →
/// precheck → 原子写盘 → changelist + diff 预览。
pub(super) fn modify_file_batch_edits(
    v: &Value,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    path: &str,
    _display_path: &str,
) -> String {
    let edits = match parse_batch_edits(v) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(false);
    let skip_precheck = v
        .get("skip_precheck")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let (original, out) = match run_batch_edits_on_snapshot(target, &edits) {
        Ok(x) => x,
        Err(e) => return e,
    };

    if dry_run {
        return batch_dry_run_preview(path, &edits, &original, out);
    }

    if let Err(e) = super::write_precheck::precheck_before_write(path, &out, skip_precheck) {
        return e;
    }

    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    match write_bytes_under_root(&base, target, out.as_bytes(), false, false) {
        Ok(()) => finish_batch_success(ctx, working_dir, path, &edits, &original, target),
        Err(e) => format!("写入文件失败: {}", e),
    }
}
