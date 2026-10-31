// ── 文件 / 归档 / 写入 / 导航族的摘要与「旧结构体反序列化边界」────────────────────
//
// 与 `fragment_args_summary.rs` 同属一个模块（`include!` 拼进同一 `mod`），拆出来只是为
// 守住 `scripts/fn-nloc-ratchet.sh` 的单文件 920 行上限——这边是「按路径 / 按补丁 / 按符号」
// 取值的一族，和那边的「按工具字段直读」分开更好读。
//
// `arg_opt_str` / `arg_str_vec` / `arg_opt_u64` 三个取值 helper 与 `arg_str` / `arg_u64` 同族，
// 统一放在 `tool_summary_args.rs`（本模块所有 fragment 共用，不专属这一族）。

// ── P2 批：archive 三件套 ──────────────────────────────────────────────────
//
// 注意：`specs/archive.inc.rs` 挂在**非默认** feature `archive-tools` 下，所以这批的零漂移
// 校验必须带 feature 跑（golden 存全集、只比对当前构建实际注册的工具）：
// `cargo test -p crabmate --features archive-tools --lib cm_tools::tools::tool_specs_registry`.

impl ToolArgsSummary for super::tool_param_types::ArchivePackArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let output = arg_str(args, "output")?;
        // `sources` 只用来数个数，但元素类型仍要校验（`arg_str_vec` 负责）：
        // 否则「模型传了数字数组」会凭空多出一行摘要。
        Some(format!(
            "pack {} items into {output}",
            arg_str_vec(args, "sources")?.len()
        ))
    }
}

impl ToolArgsSummary for super::tool_param_types::ArchiveUnpackArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let archive = arg_str(args, "archive")?;
        // 旧实现是 `Option<String>` + `.filter(|s| !s.is_empty())` + `unwrap_or(".")`：
        // **不 trim**。所以显式空白串会被原样输出（`unpack a.tar to    `），只有空串与缺键
        // 才回落 `.`。这条边界与 `path_or_dot` 的 trim 语义不同，不能合并成同一个 helper。
        let dir = match args.get("output_dir") {
            None | Some(serde_json::Value::Null) => ".",
            Some(serde_json::Value::String(raw)) if raw.is_empty() => ".",
            Some(serde_json::Value::String(raw)) => raw.as_str(),
            Some(_) => return None,
        };
        Some(format!("unpack {archive} to {dir}"))
    }
}

impl ToolArgsSummary for super::tool_param_types::ArchiveListArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        Some(format!("list archive: {}", arg_str(args, "archive")?))
    }
}

// ── P2 批：写入 / 导航族（`modify_file` / `apply_patch` / `code_nav`）─────────────

impl ToolArgsSummary for super::tool_param_types::ModifyFileArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let path = arg_str(args, "path")?;
        // `mode` **既不 trim 也不做别名归一**：旧结构体是 `Option<String>` + `unwrap_or("full")`，
        // 只有逐字等于 `replace_lines` 才走行号文案（`"replace_lines "` 会落到 else 分支）。
        let mode = match args.get("mode") {
            None => "full",
            Some(v) if v.is_null() => "full",
            Some(serde_json::Value::String(raw)) => raw.as_str(),
            Some(_) => return None,
        };
        // 行号在**两个分支之前**都校验类型：旧结构体是整体反序列化，
        // `mode: "overwrite"` 配 `start_line: "3"` 同样会让整条摘要消失。
        let start = arg_opt_u64(args, "start_line")?.unwrap_or(0);
        let end = arg_opt_u64(args, "end_line")?.unwrap_or(0);
        if mode != "replace_lines" {
            return Some(format!("modify file: {path}"));
        }
        Some(format!("replace lines {start}-{end} in {path}"))
    }
}

impl ToolArgsSummary for super::tool_param_types::ApplyPatchArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        // `patch` 用 raw 取值：空串是**合法**入参，旧实现会给「apply patch」（没有文件名可列）；
        // 用 `arg_str` 会把空串当成「没给」而吞掉整行摘要。
        let patch = arg_str_raw(args, "patch")?;
        let files = patched_files(patch);
        if files.is_empty() {
            return Some("apply patch".to_string());
        }
        Some(format!("apply patch: {}", files.join(", ")))
    }
}

/// 从 unified diff 的 `+++ ` 行取目标文件名：取第一个 token，跳过空串与 `/dev/null`，再去掉
/// `b/` / `a/` 前缀。跳过 `/dev/null` 是有意的——删除文件时补丁里只有 `+++ /dev/null`，
/// 不该被列成一个目标文件。
fn patched_files(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|line| line.strip_prefix("+++ "))
        .map(|s| s.split_whitespace().next().unwrap_or(""))
        .filter(|s| !s.is_empty() && *s != "/dev/null")
        .map(|s| {
            s.trim_start_matches("b/")
                .trim_start_matches("a/")
                .to_string()
        })
        .collect()
}

impl ToolArgsSummary for super::tool_param_types::CodeNavArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        // 四个 action 共用一条摘要通道：这里只做分发，每个 action 的文案各自一个小函数
        // （避免堆成分支丛林，lizard CCN≤10）。三个可选字段即使与 action 无关也照旧校验类型，
        // 因为旧结构体是**整体**反序列化——坏字段落在「没用到」的位置上同样不给摘要。
        let action = match args.get("action") {
            None => "",
            Some(v) if v.is_null() => "",
            Some(serde_json::Value::String(raw)) => raw.trim(),
            Some(_) => return None,
        };
        let symbol = arg_opt_str(args, "symbol")?;
        let symbols = arg_str_vec(args, "symbols")?;
        let path = arg_opt_str(args, "path")?;
        match action {
            "file_outline" => outline_line(path),
            "find_references" => references_line(symbol),
            "call_graph_sketch" => sketch_line(symbol, symbols),
            "find_symbol" => symbol_line(symbol),
            other => unknown_action_line(other, symbol),
        }
    }
}

fn outline_line(path: Option<&str>) -> Option<String> {
    let path = path.unwrap_or("").trim();
    (!path.is_empty()).then(|| format!("Rust outline: {path}"))
}

fn references_line(symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    (!s.is_empty()).then(|| format!("find references: {s}"))
}

fn symbol_line(symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    (!s.is_empty()).then(|| format!("find symbol: {s}"))
}

/// 缺 / 未知 action：中性文案，不把一次注定失败的调用显示成成功的 `find_symbol`。
fn unknown_action_line(action: &str, symbol: Option<&str>) -> Option<String> {
    let s = symbol.unwrap_or("").trim();
    if s.is_empty() {
        return None;
    }
    Some(if action.is_empty() {
        format!("code_nav: {s}")
    } else {
        format!("code_nav {action}: {s}")
    })
}

fn sketch_line(symbol: Option<&str>, symbols: Vec<&str>) -> Option<String> {
    let mut v: Vec<&str> = symbols
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(s) = symbol.map(str::trim)
        && !s.is_empty()
    {
        v.push(s);
    }
    v.sort_unstable();
    v.dedup();
    if v.is_empty() {
        return None;
    }
    const MAX: usize = 3;
    let mut out = format!(
        "impact sketch: {}",
        v.iter().take(MAX).copied().collect::<Vec<_>>().join(", ")
    );
    if v.len() > MAX {
        out.push_str(&format!(" +{}", v.len() - MAX));
    }
    Some(out)
}

#[cfg(test)]
mod archive_family_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::{
        ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs,
    };
    use serde_json::json;

    // 这批的旧实现同样是「字段子集结构体 + 反序列化失败即无摘要」，所以钉边界而不是快乐路径。

    #[test]
    fn pack_counts_sources_and_requires_output() {
        let v = json!({ "output": " out.tar ", "sources": ["a", "b", "c"] });
        assert_eq!(
            ArchivePackArgs::summary_line(&v).as_deref(),
            Some("pack 3 items into out.tar")
        );
        assert!(ArchivePackArgs::summary_line(&json!({ "output": "  " })).is_none());
    }

    /// `sources` 缺键 ⇒ 0 项；`null` / 非数组 / 含非字符串元素 ⇒ 无摘要
    /// （旧 `#[serde(default)] Vec<String>` 宽容缺键、严格于类型，两条边界都在）。
    #[test]
    fn pack_sources_keep_the_old_deserialize_boundary() {
        assert_eq!(
            ArchivePackArgs::summary_line(&json!({ "output": "out.tar" })).as_deref(),
            Some("pack 0 items into out.tar")
        );
        assert!(
            ArchivePackArgs::summary_line(&json!({ "output": "o.tar", "sources": null })).is_none()
        );
        assert!(
            ArchivePackArgs::summary_line(&json!({ "output": "o.tar", "sources": "a,b" }))
                .is_none()
        );
        assert!(
            ArchivePackArgs::summary_line(&json!({ "output": "o.tar", "sources": ["a", 1] }))
                .is_none()
        );
    }

    /// `output_dir` 缺键与空串回落 `.`，但**不 trim**：显式空白串原样输出。
    #[test]
    fn unpack_defaults_to_dot_but_does_not_trim_explicit_whitespace() {
        assert_eq!(
            ArchiveUnpackArgs::summary_line(&json!({ "archive": " a.tar " })).as_deref(),
            Some("unpack a.tar to .")
        );
        assert_eq!(
            ArchiveUnpackArgs::summary_line(&json!({ "archive": "a.tar", "output_dir": "" }))
                .as_deref(),
            Some("unpack a.tar to .")
        );
        assert_eq!(
            ArchiveUnpackArgs::summary_line(&json!({ "archive": "a.tar", "output_dir": "  " }))
                .as_deref(),
            Some("unpack a.tar to   ")
        );
        assert_eq!(
            ArchiveUnpackArgs::summary_line(&json!({ "archive": "a.tar", "output_dir": " out " }))
                .as_deref(),
            Some("unpack a.tar to  out ")
        );
    }

    #[test]
    fn list_requires_archive() {
        assert_eq!(
            ArchiveListArgs::summary_line(&json!({ "archive": " pkg.zip ", "max_entries": 5 }))
                .as_deref(),
            Some("list archive: pkg.zip")
        );
        assert!(ArchiveListArgs::summary_line(&json!({ "verbose": true })).is_none());
    }
}

#[cfg(test)]
mod code_nav_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::CodeNavArgs;
    use serde_json::json;

    // 从旧的 fragment（走 `serde_json::from_value::<CodeNavSummaryArgs>`）搬到 Args 同侧；
    // 顺手把「类型不符」的边界也钉住——那正是手取 `&Value` 时最容易放宽的一条。

    #[test]
    fn covers_all_four_actions() {
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "find_symbol", "symbol": "run_tool"}))
                .as_deref(),
            Some("find symbol: run_tool")
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "find_references", "symbol": "run_tool"}))
                .as_deref(),
            Some("find references: run_tool")
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "file_outline", "path": "src/a.rs"}))
                .as_deref(),
            Some("Rust outline: src/a.rs")
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({
                "action": "call_graph_sketch",
                "symbols": ["b", "a"],
                "symbol": "c"
            }))
            .as_deref(),
            Some("impact sketch: a, b, c")
        );
    }

    #[test]
    fn blank_inputs_yield_no_line() {
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "find_symbol"})),
            None
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "file_outline"})),
            None
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "call_graph_sketch"})),
            None
        );
    }

    #[test]
    fn unknown_action_is_not_labelled_as_find_symbol() {
        // 缺 / 未知 action 曾复用 `find symbol:` 文案，与真实失败原因不符。
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"action": "nope", "symbol": "x"})).as_deref(),
            Some("code_nav nope: x")
        );
        assert_eq!(
            CodeNavArgs::summary_line(&json!({"symbol": "x"})).as_deref(),
            Some("code_nav: x")
        );
    }

    /// 与 action 无关的字段类型不符，同样不给摘要：旧结构体是**整体**反序列化，
    /// `symbol: 1` 会让一次本来合法的 `file_outline` 调用也失去摘要行。
    #[test]
    fn wrong_typed_field_anywhere_drops_the_line() {
        assert!(
            CodeNavArgs::summary_line(
                &json!({"action": "file_outline", "path": "src/a.rs", "symbol": 1})
            )
            .is_none()
        );
        assert!(
            CodeNavArgs::summary_line(
                &json!({"action": "find_symbol", "symbol": "a", "symbols": ["x", 1]})
            )
            .is_none()
        );
        assert!(
            CodeNavArgs::summary_line(
                &json!({"action": "find_symbol", "symbol": "a", "path": [1]})
            )
            .is_none()
        );
    }

    /// `call_graph_sketch` 去重排序后最多列 3 个，其余折成 ` +N`。
    #[test]
    fn sketch_truncates_to_three_and_counts_the_rest() {
        assert_eq!(
            CodeNavArgs::summary_line(&json!({
                "action": "call_graph_sketch",
                "symbols": ["e", "d", "c", "b", "a", "a"]
            }))
            .as_deref(),
            Some("impact sketch: a, b, c +2")
        );
    }
}

#[cfg(test)]
mod write_family_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::{ApplyPatchArgs, ModifyFileArgs};
    use serde_json::json;

    #[test]
    fn modify_file_covers_full_and_replace_lines() {
        assert_eq!(
            ModifyFileArgs::summary_line(&json!({"path": " a.rs ", "content": "x"})).as_deref(),
            Some("modify file: a.rs")
        );
        assert_eq!(
            ModifyFileArgs::summary_line(&json!({"path": "a.rs", "mode": "overwrite"})).as_deref(),
            Some("modify file: a.rs")
        );
        assert_eq!(
            ModifyFileArgs::summary_line(
                &json!({"path": "a.rs", "mode": "replace_lines", "start_line": 3, "end_line": 5})
            )
            .as_deref(),
            Some("replace lines 3-5 in a.rs")
        );
        // 行号缺省回落 0（旧 `unwrap_or(0)`）。
        assert_eq!(
            ModifyFileArgs::summary_line(&json!({"path": "a.rs", "mode": "replace_lines"}))
                .as_deref(),
            Some("replace lines 0-0 in a.rs")
        );
    }

    /// `mode` **不 trim**：带空格即不匹配 `replace_lines`；类型不符则整条摘要消失。
    #[test]
    fn modify_file_mode_is_exact_match_and_typed() {
        assert_eq!(
            ModifyFileArgs::summary_line(&json!({
                "path": "a.rs",
                "mode": "replace_lines ",
                "start_line": 1,
                "end_line": 2
            }))
            .as_deref(),
            Some("modify file: a.rs")
        );
        assert!(ModifyFileArgs::summary_line(&json!({"path": "a.rs", "mode": 7})).is_none());
        assert!(
            ModifyFileArgs::summary_line(
                &json!({"path": "a.rs", "mode": "replace_lines", "start_line": "3"})
            )
            .is_none()
        );
        assert!(ModifyFileArgs::summary_line(&json!({"path": "   "})).is_none());
    }

    /// 行号类型不符在**任何 mode** 下都不给摘要：旧结构体是整体反序列化，
    /// `mode: "overwrite"` + `start_line: "3"` 也会让摘要消失。
    #[test]
    fn modify_file_wrong_typed_line_numbers_drop_the_line_in_any_mode() {
        assert!(
            ModifyFileArgs::summary_line(
                &json!({"path": "a.rs", "mode": "overwrite", "start_line": "3"})
            )
            .is_none()
        );
        assert!(ModifyFileArgs::summary_line(&json!({"path": "a.rs", "end_line": 1.5})).is_none());
    }

    #[test]
    fn apply_patch_lists_targets() {
        let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-a\n+b\n--- /dev/null\n+++ b/y/z.txt\n";
        assert_eq!(
            ApplyPatchArgs::summary_line(&json!({"patch": patch})).as_deref(),
            Some("apply patch: x.rs, y/z.txt")
        );
        assert_eq!(
            ApplyPatchArgs::summary_line(&json!({"patch": "no headers here"})).as_deref(),
            Some("apply patch")
        );
    }

    /// 空串是**合法**入参：旧实现给「apply patch」。若改用 `arg_str` 取值，空串会被当成
    /// 「没给」而整行摘要消失——这条就是防这个回归的。
    #[test]
    fn apply_patch_empty_patch_still_has_a_line() {
        assert_eq!(
            ApplyPatchArgs::summary_line(&json!({"patch": ""})).as_deref(),
            Some("apply patch")
        );
        assert!(ApplyPatchArgs::summary_line(&json!({})).is_none());
        assert!(ApplyPatchArgs::summary_line(&json!({"patch": 1})).is_none());
    }

    /// 删除文件的补丁里只有 `+++ /dev/null`，不该被列成一个目标文件。
    #[test]
    fn apply_patch_ignores_dev_null_target() {
        assert_eq!(
            ApplyPatchArgs::summary_line(&json!({"patch": "--- a/old.rs\n+++ /dev/null\n"}))
                .as_deref(),
            Some("apply patch")
        );
    }
}
