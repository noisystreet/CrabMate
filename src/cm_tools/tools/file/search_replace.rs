//! `search_replace`：单文件搜索替换（由 `mutate.rs` 拆出，受单文件 920 行上限所迫）。
//!
//! 三条不变量：
//! - **默认要求 `search` 唯一匹配**：锚点重复时服务端无法知道要改哪一处，静默全改的代价
//!   只有在事后才被发现，因此默认拒写并列出命中位置；
//! - **替换在原文上左到右不重叠推进**（`str::replacen` / `Regex::replacen`），「前 N 处」
//!   不会命中上一轮刚插入的文本；
//! - **写盘走统一提交通道**（tmp → 继承元数据 → `fsync` → revision 校验 → `rename`），
//!   与 `modify_file` / `apply_patch` 同一实现，不再截断式写入。

use regex::{Regex, RegexBuilder};
use std::ops::Range;
use std::path::{Path, PathBuf};

use super::path::{PathAccess, path_for_tool_display};
use super::write_precheck;
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::tool_param_types::SearchReplaceArgs;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;
use crate::cm_names as tool_names;
// ── search_replace ──────────────────────────────────────────

/// `search_replace` 的替换范围。
///
/// 旧默认是「不传 `max_replacements` 就替换全部」：`unwrap_or(0)` 把「没声明」与「不限次数」
/// 压成同一个值，于是「只想改一处」会静默命中全文（`use …;`、`return x;` 这类短锚点在真实
/// 文件里几乎必然重复）。现在把默认反过来——**没声明就要求唯一匹配**，多处即拒写并列出
/// 位置，由调用方显式选择「前 N 处」或「全部」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceScope {
    /// 未显式声明：`search` 必须只匹配 1 处。
    Unique,
    /// 只替换前 N 处（`N >= 1`）。
    First(usize),
    /// 全部替换（`max_replacements=0` 或 `replace_all=true`）。
    All,
}

/// 歧义拒写时最多列出的命中位置数。
const AMBIGUOUS_SITES_SHOWN: usize = 10;

/// 由 `max_replacements` / `replace_all` 解析替换范围；两者语义冲突时报错而不是猜一个。
fn resolve_replace_scope(args: &SearchReplaceArgs) -> Result<ReplaceScope, String> {
    match (args.max_replacements, args.replace_all) {
        (None, false) => Ok(ReplaceScope::Unique),
        (None, true) => Ok(ReplaceScope::All),
        // `max_replacements=0` 是历史「全部替换」的写法，保留为显式逃生门。
        (Some(0), _) => Ok(ReplaceScope::All),
        (Some(n), false) => Ok(ReplaceScope::First(
            usize::try_from(n).unwrap_or(usize::MAX),
        )),
        (Some(n), true) => Err(format!(
            "max_replacements={n}（限量）与 replace_all=true（全部）冲突，请只保留一个：\
             要限量传 max_replacements=N，要全部传 replace_all=true 或 max_replacements=0"
        )),
    }
}

/// 收集原文中全部匹配区间（不重叠、按出现顺序）。
///
/// 正则模式顺带回传编译好的 [`Regex`]：替换要靠它展开 `$1` / `${name}`，
/// 先数一遍再编译一遍是白花钱。
fn search_replace_spans(
    content: &str,
    search: &str,
    is_regex: bool,
) -> Result<(Vec<Range<usize>>, Option<Regex>), String> {
    if !is_regex {
        let spans = content
            .match_indices(search)
            .map(|(start, m)| start..start + m.len())
            .collect();
        return Ok((spans, None));
    }
    let re = RegexBuilder::new(search)
        .build()
        .map_err(|e| format!("正则表达式无效：{}", e))?;
    let spans = re.find_iter(content).map(|m| m.start()..m.end()).collect();
    Ok((spans, Some(re)))
}

/// 执行替换，返回 `(新内容, 实际替换处数)`。
///
/// `total` 是原文命中总数，用于把「声明的上限」裁到实际数量。**先数总数、再一次性替换**：
/// 旧实现在 `max_replacements > 0` 时循环 `find` **改写后的字符串**，会命中上一轮刚插入的
/// 文本（`"aXa"` 把 `a` 换成 `aa`，第 2 轮命中的是新插入的 `a`），且每轮 `format!` 拼整串
/// 是二次复杂度。`str::replacen` / `Regex::replacen` 都按原文左到右不重叠推进，语义正确且线性。
fn apply_search_replace_inner(
    content: &str,
    search: &str,
    replace: &str,
    re: Option<&Regex>,
    total: usize,
    limit: Option<usize>,
) -> (String, usize) {
    let replaced = match limit {
        None => total,
        Some(n) => total.min(n),
    };
    let new = match (re, limit) {
        (Some(re), None) => re.replace_all(content, replace).to_string(),
        (Some(re), Some(n)) => re.replacen(content, n, replace).to_string(),
        (None, None) => content.replace(search, replace),
        (None, Some(n)) => content.replacen(search, replace, n),
    };
    (new, replaced)
}

/// 把匹配起始偏移折算成 1-based 行号（只看前 `at_most` 处，避免病态文件上逐处扫全量）。
fn span_line_numbers(content: &str, spans: &[Range<usize>], at_most: usize) -> Vec<usize> {
    spans
        .iter()
        .take(at_most)
        .map(|s| content[..s.start].bytes().filter(|b| *b == b'\n').count() + 1)
        .collect()
}

/// 命中行号摘要（如 `L12, L48`），超出 [`AMBIGUOUS_SITES_SHOWN`] 处以 `…（共 N 处）` 收尾。
fn hit_lines(content: &str, spans: &[Range<usize>]) -> String {
    let shown = spans.len().min(AMBIGUOUS_SITES_SHOWN);
    let mut out = span_line_numbers(content, spans, shown)
        .into_iter()
        .map(|n| format!("L{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    if spans.len() > shown {
        out.push_str(&format!(" …（共 {} 处）", spans.len()));
    }
    out
}

/// 单行内容摘要：过长时按字符截断（按字节切会切坏多字节字符）。
fn line_preview(line: &str, max_chars: usize) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max_chars).collect();
    format!("{head}…")
}

/// `search` 多处命中且调用方未声明范围时的拒写正文。
///
/// 拒绝而不是「替换全部」是这里唯一诚实的选项：锚点重复时服务端无法知道要改哪一处，
/// 静默全改的代价由调用方（往往是模型）在事后才发现。列出位置 + 两条改法让自纠错成为一步。
fn ambiguous_match_message(
    display: &str,
    content: &str,
    spans: &[Range<usize>],
) -> String {
    let total = spans.len();
    let mut msg = format!(
        "错误：search 在 {display} 匹配 {total} 处，无法唯一确定要替换哪一处，本次未写入任何内容（{}）。",
        write_precheck::NOT_WRITTEN_HINT
    );
    msg.push_str("\n命中位置：");
    let shown = total.min(AMBIGUOUS_SITES_SHOWN);
    for line_no in span_line_numbers(content, spans, shown) {
        let text = content.lines().nth(line_no - 1).unwrap_or("");
        msg.push_str(&format!("\n  L{line_no}: {}", line_preview(text, 80)));
    }
    if total > shown {
        msg.push_str(&format!("\n  …（共 {total} 处，仅列前 {shown} 处）"));
    }
    msg.push_str(
        "\n改法（二选一）：\n\
         1) 补足上下文让 search 唯一——把锚点连同上下 1~2 行一起写进 search（推荐）；\n\
         2) 显式声明替换范围：max_replacements=N 只改前 N 处，或 replace_all=true / max_replacements=0 全部替换。",
    );
    msg
}

/// 一次替换的上下文：成功正文与 dry-run 预览共用，避免两个函数各摊 6~7 个位置参数。
struct SearchReplaceOutcome<'a> {
    /// 面向模型/用户的展示路径（相对工作区）。
    display: &'a str,
    /// diff 头里的相对路径（`--- a/<rel_path>`）。
    rel_path: &'a str,
    /// 替换前的原文。
    content: &'a str,
    /// 原文全部命中区间。
    spans: &'a [Range<usize>],
    /// 原文命中总数。
    total: usize,
    /// 本次实际替换处数。
    replaced: usize,
    search: &'a str,
    replace: &'a str,
}

fn search_replace_success_body(o: &SearchReplaceOutcome<'_>) -> String {
    let head = format!(
        "已替换 {} 处匹配（\"{}\" → \"{}\"）：{}",
        o.replaced, o.search, o.replace, o.display
    );
    if o.replaced <= 1 {
        return head;
    }
    // 多处替换必须在首行就刺眼：旧正文只写「已替换 N 处」，模型据此继续编辑时并不知道自己改多了。
    format!(
        "⚠ 多处替换：search 共命中 {} 处，本次改了 {} 处\n{}\n  命中行：{}",
        o.total,
        o.replaced,
        head,
        hit_lines(o.content, &o.spans[..o.replaced])
    )
}

fn search_replace_dry_run_preview(o: &SearchReplaceOutcome<'_>, new_content: &str) -> String {
    let mut body = format!(
        "预览（dry-run）：在 {} 找到 {} 处匹配，本次将替换 {} 处",
        o.display, o.total, o.replaced
    );
    if o.total > 1 {
        body.push_str(&format!(
            "\n⚠ 多处匹配：命中行 {}",
            hit_lines(o.content, &o.spans[..o.replaced])
        ));
    }
    body.push_str("\n设置 dry_run=false 以实际写入；下方为本次替换的 unified diff");
    // 与写盘结果共用同一个 diff 渲染：旧预览按行 zip 对比，替换改变行数后其后每行都被判成
    // 「变了」，20 行上限迅速耗尽、结论失真。
    format_tool_output_with_write_diff_preview(
        tool_names::SEARCH_REPLACE,
        true,
        body,
        vec![WriteDiffFileState {
            rel_path: o.rel_path.to_string(),
            before: Some(o.content.to_string()),
            after: Some(new_content.to_string()),
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

fn parse_search_replace_args(raw_args: &serde_json::Value) -> Result<SearchReplaceArgs, String> {
    crate::cm_tools::tools::parse_args_from_value(raw_args.clone())
}

fn search_replace_path_and_query(args: &SearchReplaceArgs) -> Result<(String, String), String> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    // **保留 `search` 原文**（含首行前导空白），只拦真正空串。
    // 若对 `search` 取 `trim()`，会剥掉首行缩进：锚点变短后按子串替换，文件原缩进前缀与
    // `replace` 自带缩进叠加 → 写出「缩进翻倍」的坏文件（Python 等缩进敏感语言尤甚）。
    // 同理这里不能用 `trim().is_empty()`：`"\n\n"`（替换空行）、`"    "`（统一缩进）都是
    // 合法锚点，`trim()` 会把它们判成「没传 search」。
    if args.search.is_empty() {
        return Err("缺少 search 参数".to_string());
    }
    Ok((path, args.search.clone()))
}

/// 写盘前闸门：语法预检（失败返回 `校验失败（CODE）…` 拒写消息）。
///
/// 写盘口径已统一为「一个旋钮」——显式 `dry_run=false` 即写，不再要求 `confirm=true`；
/// `confirm` 字段保留以兼容既有调用（继续传也不会被拒）。
#[allow(clippy::result_large_err)]
fn search_replace_precheck(
    skip_precheck: bool,
    path: &str,
    new_content: &str,
) -> Result<(), ToolError> {
    super::write_precheck::precheck_before_write(path, new_content, skip_precheck)
        .map_err(|e| ToolError::write_precheck_failed(tool_names::SEARCH_REPLACE, e))
}

#[allow(clippy::result_large_err)]
fn load_search_replace_file_bytes(
    access: &PathAccess<'_>,
    path: &str,
) -> Result<(PathBuf, String), ToolError> {
    let target = access.for_read(path).map_err(ToolError::path_resolve_failed)?;
    if !target.is_file() {
        return Err(ToolError::invalid_args(format!("错误：{} 不是文件", path)));
    }
    let content = std::fs::read_to_string(&target).map_err(|e| {
        ToolError::external_code("search_replace_failed", format!("读取文件失败：{}", e))
    })?;
    const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
    if content.len() > MAX_FILE_SIZE {
        return Err(ToolError::invalid_args(format!(
            "错误：文件过大（{} 字节，上限 4MiB）",
            content.len()
        )));
    }
    Ok((target, content))
}

pub fn search_replace(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    search_replace_try(args_json, &PathAccess::new(working_dir), ctx).unwrap_or_else(|e| e.message)
}

/// 把「首行 `错误[CODE]`」的正文还原成带精确码的 [`ToolError`]。
///
/// `parse_tool_output` 只认**首行**标记，所以调用方不能给这类正文加 `写入文件失败：` 前缀。
fn search_replace_typed_error(body: String) -> ToolError {
    let parsed = crate::cm_tools::tool_result::parse_tool_output(tool_names::SEARCH_REPLACE, &body);
    ToolError::from_parsed(tool_names::SEARCH_REPLACE, &parsed, body)
}

/// 写前 CAS：显式 `base_revision` 始终校验；本轮 `read_file` 有绑定就顺带比对；
/// **两者都没有则放行**。
///
/// 与行号类编辑的差别是有意的：`search_replace` 按文本锚定、不依赖行号快照，正是
/// `modify_file` 报 `missing_revision_baseline` 时推荐的替代路径——若也强求基线，
/// 那条推荐就自相矛盾了（没读过文件就没法用锚点改）。
#[allow(clippy::result_large_err)]
fn search_replace_verify_revision(
    raw_args: &serde_json::Value,
    target: &Path,
    ctx: &ToolContext<'_>,
) -> Result<(), ToolError> {
    let bound = ctx
        .read_file_turn_cache
        .and_then(|c| c.last_read_revision(target));
    if super::revision::base_revision_arg(raw_args).is_none() && bound.is_none() {
        return Ok(());
    }
    super::revision::verify_write_revision(raw_args, target, ctx, true)
        .map_err(search_replace_typed_error)
}

/// 写盘：走与 `modify_file` / `apply_patch` 同一条提交通道（同目录临时文件 → 继承属主/权限 →
/// `fsync` → revision 校验 → `rename`），不再用截断式 `write`（中途崩溃会留下半截文件）。
///
/// `auto_bind=false`：轮次绑定已由 [`search_replace_verify_revision`] 处理，这里只保留紧贴
/// `rename` 的显式 `base_revision` 校验（两次不是重复劳动，后者覆盖「预览/计算 → rename」的窗口）。
#[allow(clippy::result_large_err)]
fn search_replace_commit(
    target: &Path,
    new_content: &str,
    raw_args: &serde_json::Value,
    ctx: &ToolContext<'_>,
) -> Result<(), ToolError> {
    match super::commit_guard::write_bytes_with_revision_guard(
        target,
        new_content.as_bytes(),
        raw_args,
        ctx,
        false,
    ) {
        Ok(()) => Ok(()),
        Err(e) if e.starts_with("错误[") => Err(search_replace_typed_error(e)),
        Err(e) => Err(ToolError::external_code(
            "search_replace_failed",
            format!("写入文件失败：{}", e),
        )),
    }
}

/// 一次 `search_replace` 调用的入参解析结果。
///
/// 原始 JSON 与类型化视图都要：`base_revision` / `allow_stale_revision` 由写侧守卫直接读原值，
/// 而字段本身走 `SearchReplaceArgs`——分两处各解析一遍 `args_json` 是白花钱。
struct SearchReplacePlan {
    raw_args: serde_json::Value,
    path: String,
    search: String,
    replace: String,
    is_regex: bool,
    dry_run: bool,
    skip_precheck: bool,
    scope: ReplaceScope,
}

#[allow(clippy::result_large_err)]
fn prepare_search_replace(args_json: &str) -> Result<SearchReplacePlan, ToolError> {
    let raw_args =
        crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let args = parse_search_replace_args(&raw_args).map_err(ToolError::invalid_args)?;
    let (path, search) = search_replace_path_and_query(&args).map_err(ToolError::invalid_args)?;
    let scope = resolve_replace_scope(&args).map_err(ToolError::invalid_args)?;
    Ok(SearchReplacePlan {
        raw_args,
        path,
        search,
        replace: args.replace,
        is_regex: args.regex,
        dry_run: args.dry_run,
        skip_precheck: args.skip_precheck,
        scope,
    })
}

/// 无匹配 / 范围判定：返回 `replacen` 的 `limit`（`None` = 全部替换）。
///
/// 两种失败都属「调用方给的信息不够」，必须在写盘路径之前拒掉：没有匹配说明锚点写错，
/// 多处匹配说明锚点不唯一——后者正是「只想改一处却全改了」的来源，因此默认拒写。
#[allow(clippy::result_large_err)]
fn resolve_replace_limit(
    plan: &SearchReplacePlan,
    content: &str,
    spans: &[Range<usize>],
    display: &str,
) -> Result<Option<usize>, ToolError> {
    if spans.is_empty() {
        return Err(ToolError::invalid_args(format!(
            "未找到匹配：\"{}\" 在 {}",
            plan.search, plan.path
        )));
    }
    match plan.scope {
        ReplaceScope::Unique if spans.len() > 1 => Err(ToolError::invalid_args(
            ambiguous_match_message(display, content, spans),
        )),
        ReplaceScope::Unique => Ok(Some(1)),
        ReplaceScope::First(n) => Ok(Some(n)),
        ReplaceScope::All => Ok(None),
    }
}

/// 写盘收尾：语法预检 → 原子提交 → 记录会话变更集 → 组装成功正文（含 unified diff 预览）。
#[allow(clippy::result_large_err)]
fn search_replace_persist(
    plan: &SearchReplacePlan,
    outcome: &SearchReplaceOutcome<'_>,
    working_dir: &Path,
    target: &Path,
    new_content: String,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    search_replace_precheck(plan.skip_precheck, &plan.path, &new_content)?;
    search_replace_commit(target, &new_content, &plan.raw_args, ctx)?;
    let body = search_replace_success_body(outcome);
    let before = outcome.content.to_string();
    record_file_state_after_write(
        ctx.workspace_changelist,
        working_dir,
        &plan.path,
        Some(before.clone()),
    );
    Ok(format_tool_output_with_write_diff_preview(
        tool_names::SEARCH_REPLACE,
        true,
        body,
        vec![WriteDiffFileState {
            rel_path: plan.path.clone(),
            before: Some(before),
            after: Some(new_content),
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    ))
}

/// [`search_replace`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn search_replace_try(
    args_json: &str,
    access: &PathAccess<'_>,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let working_dir = access.root();
    let plan = prepare_search_replace(args_json)?;
    let (target, content) = load_search_replace_file_bytes(access, &plan.path)?;
    let (spans, re) = search_replace_spans(&content, &plan.search, plan.is_regex)
        .map_err(ToolError::invalid_args)?;
    let display = path_for_tool_display(working_dir, &target, Some(&plan.path));
    let limit = resolve_replace_limit(&plan, &content, &spans, &display)?;
    let (new_content, replaced) = apply_search_replace_inner(
        &content,
        &plan.search,
        &plan.replace,
        re.as_ref(),
        spans.len(),
        limit,
    );
    let outcome = SearchReplaceOutcome {
        display: &display,
        rel_path: &plan.path,
        content: &content,
        spans: &spans,
        total: spans.len(),
        replaced,
        search: &plan.search,
        replace: &plan.replace,
    };

    // 显式 `base_revision` 在**预览阶段也校验**（与 `structured_patch` 一致）：预览若基于过期
    // 快照，应当立刻报错，而不是给出一份「看起来安全」的 diff 让人据此决定写盘。
    search_replace_verify_revision(&plan.raw_args, &target, ctx)?;

    if plan.dry_run {
        return Ok(search_replace_dry_run_preview(&outcome, &new_content));
    }
    search_replace_persist(&plan, &outcome, working_dir, &target, new_content, ctx)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{test_ctx, write};
    use super::*;
    use std::fs;

    /// search_replace 替换出 Python 缩进错误 → 拒写（写盘前校验），原文件不变。
    #[test]
    fn search_replace_py_bad_indent_rejected_by_precheck() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    return 1\n");
        let before = fs::read_to_string(tmp.path().join("app.py")).unwrap();
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"    return 1","replace":"x = 1\nreturn 2","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("校验失败（PY_SYNTAX_ERROR）"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("app.py")).unwrap(),
            before,
            "校验失败应拒写，文件不得被修改"
        );
    }

    /// search_replace `skip_precheck=true` 绕过校验并写盘。
    #[test]
    fn search_replace_skip_precheck_writes() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    return 1\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"return 1","replace":"x = 1","dry_run":false,"confirm":true,"skip_precheck":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(!out.contains("校验失败"), "{out}");
        assert!(out.contains("已替换"), "{out}");
        assert!(fs::read_to_string(tmp.path().join("app.py"))
            .unwrap()
            .contains("x = 1"));
    }

    /// 缩进敏感回归：`search` 首行带缩进时按字面量替换，不得出现「缩进翻倍」。
    /// 修复前 `search` 被 `trim()` 剥掉首行缩进，与 `replace` 自带缩进叠加，且叠加后语法常仍合法（`write_precheck` 拦不住）。
    /// 另外 `replace` 只改缩进层级也可精确生效（`search` 不再被剥掉前导空白）。
    #[test]
    fn search_replace_keeps_indentation_of_indented_anchor() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    if x:\n        return 1\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"        return 1","replace":"        return 2","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("app.py")).unwrap(),
            "def f():\n    if x:\n        return 2\n",
            "应保持原缩进（8 空格），而非翻倍"
        );
    }

    /// 复现：成功正文内嵌 search/replace 原文；若原文含「失败」/「超时」，
    /// 不应被工具结果状态启发式误判为失败（否则报错但文件已写入）。
    #[test]
    fn search_replace_success_with_failure_word_in_body_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "old\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"old","replace":"写入文件失败：x","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "写入文件失败：x\n",
            "写盘应已成功"
        );
        let r = crate::cm_tools::tool_result::ToolResult::from_output("search_replace", out);
        assert!(
            r.ok,
            "文件已成功写入，工具结果不应判失败: error_code={:?}",
            r.error_code
        );
    }

    // ── search_replace 替换范围 / 唯一性守卫 ──────────────────────

    /// 默认（不传 `max_replacements` / `replace_all`）：多处命中拒写，并列出命中位置。
    #[test]
    fn search_replace_ambiguous_match_is_rejected_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "let x = 1;\nkeep\nlet x = 1;\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"let x = 1;","replace":"let x = 2;","dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("匹配 2 处"), "{out}");
        assert!(out.contains("L1:"), "应列出命中行号: {out}");
        assert!(out.contains("L3:"), "应列出命中行号: {out}");
        assert!(out.contains("未写盘"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "let x = 1;\nkeep\nlet x = 1;\n",
            "歧义拒写不得改动文件"
        );
    }

    /// `max_replacements=0` 与 `replace_all=true` 都表示「显式全部替换」，并回报多处警示。
    #[test]
    fn search_replace_replace_all_forms_replace_every_match() {
        for extra in [r#""max_replacements":0,"#, r#""replace_all":true,"#] {
            let tmp = tempfile::tempdir().unwrap();
            write(tmp.path(), "a.txt", "x\nx\nx\n");
            let ctx = test_ctx(tmp.path());
            let out = search_replace(
                &format!(r#"{{"path":"a.txt","search":"x","replace":"y",{extra}"dry_run":false}}"#),
                tmp.path(),
                &ctx,
            );
            assert!(out.contains("已替换 3 处匹配"), "{extra} {out}");
            assert!(out.contains("多处替换"), "多处替换应首行警示: {extra} {out}");
            assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "y\ny\ny\n");
        }
    }

    /// `max_replacements=N` 只改前 N 处，且**不会命中上一轮刚插入的文本**。
    ///
    /// 回归：旧实现循环 `find` 改写后的字符串，`"aXa"` 把 `a` 换成 `aa` 时第 2 轮命中新插入的
    /// `a`，得到 `"aaaXa"`（第二处原文 `a` 根本未替换）。
    #[test]
    fn search_replace_first_n_does_not_rescan_inserted_text() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "aXa\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"a","replace":"aa","max_replacements":2,"dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换 2 处匹配"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "aaXaa\n",
            "应替换两处原文 a，而不是替换刚插入的 a"
        );
    }

    /// 正则路径保留 crate 的 `$1` 展开（限量替换同样生效）。
    #[test]
    fn search_replace_regex_limit_keeps_capture_expansion() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "a@ b@\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"(\\w+)@","replace":"$1:","regex":true,"max_replacements":1,"dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换 1 处匹配"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "a: b@\n",
            "只替换第一处，且 $1 应展开为捕获组"
        );
    }

    /// `max_replacements`（限量）与 `replace_all=true`（全部）同时给出时直接报冲突。
    #[test]
    fn search_replace_scope_conflict_is_invalid_input() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "x\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"x","replace":"y","max_replacements":1,"replace_all":true,"dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("冲突"), "{out}");
        assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "x\n");
    }

    /// 纯空白锚点（空行、缩进）是合法输入，不能被当成「没传 search」。
    #[test]
    fn search_replace_whitespace_only_search_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "a\n\nb\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"\n\n","replace":"\n","dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换 1 处匹配"), "{out}");
        assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "a\nb\n");
    }

    /// dry-run 预览走统一 diff 渲染（`--- a/…` / `+++ b/…`），不再逐行 zip 对比。
    #[test]
    fn search_replace_dry_run_renders_unified_diff() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "old\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"old","replace":"new"}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("--- a/a.txt"), "{out}");
        assert!(out.contains("+++ b/a.txt"), "{out}");
        assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "old\n");
    }

    /// 显式 `base_revision` 与当前内容不一致 → 拒写并保留精确错误码。
    #[test]
    fn search_replace_stale_base_revision_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "old\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"old","replace":"new","base_revision":"deadbeef","dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("错误[stale_revision]"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "old\n",
            "CAS 失败不得写入"
        );
    }

}
