// ── git（action 选择器） ──────────────────────────────────────

/// `git` 的摘要行：一个 `impl` 覆盖全部 action（原 10 个 `git_*` 工具里 `git_diff` 用动态文案、
/// 其余用静态文案；合并成 `git` 后统一在这里按 action 出文案）。
///
/// 旧 `GitSummaryArgs` 的每个字段都带 `#[serde(default)]`：缺键宽容，类型不符整体失败
/// （= 没有摘要）。所以七个字段**一律先取值**，不因为当前 action 用不到就跳过校验。
impl ToolArgsSummary for super::tool_param_types::GitArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action = arg_str_default(args, "action")?;
        let mode = arg_opt_str(args, "mode")?;
        let path = arg_opt_str(args, "path")?;
        let stat = arg_opt_bool(args, "stat")?;
        let name_only = arg_opt_bool(args, "name_only")?;
        let base = arg_opt_str(args, "base")?;
        let rev = arg_opt_str(args, "rev")?;
        let action = action.trim();
        Some(match action {
            // `diff` 保留合并前的动态文案（flags / scope / path）。
            "diff" => git_diff_line(mode, path, stat, name_only, base),
            "show" => git_show_line(rev),
            "blame" => git_path_line("git blame", path),
            "file_history" => git_path_line("git file history", path),
            // 固定文案的只读查询（与合并前的 `ToolSummaryKind::Static` 对齐）；
            // 缺 action 或未识别的 action 回落工具名，避免把拼错的 `nope` 当卡片标题显示。
            other => git_static_label(other).to_string(),
        })
    }
}

/// `git diff`：flags 优先 `--stat`，其次 `--name-only`；`base` 存在时作用域是 `base...HEAD`，
/// 否则用 `mode`（**不 trim**，缺省 `working`——`"staged "` 会连空格一起显示）。
fn git_diff_line(
    mode: Option<&str>,
    path: Option<&str>,
    stat: Option<bool>,
    name_only: Option<bool>,
    base: Option<&str>,
) -> String {
    let flags = if stat.unwrap_or(false) {
        " --stat"
    } else if name_only.unwrap_or(false) {
        " --name-only"
    } else {
        ""
    };
    let scope = match base.map(str::trim).filter(|s| !s.is_empty()) {
        Some(base) => format!("{base}...HEAD"),
        None => mode.unwrap_or("working").to_string(),
    };
    let path = path.unwrap_or("").trim();
    if path.is_empty() {
        format!("git diff{flags} ({scope})")
    } else {
        format!("git diff{flags} ({scope}): {path}")
    }
}

fn git_show_line(rev: Option<&str>) -> String {
    let rev = rev.unwrap_or("").trim();
    if rev.is_empty() {
        "git show HEAD".to_string()
    } else {
        format!("git show {rev}")
    }
}

fn git_path_line(verb: &str, path: Option<&str>) -> String {
    let path = path.unwrap_or("").trim();
    if path.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {path}")
    }
}

fn git_static_label(action: &str) -> &'static str {
    match action {
        "status" => "git status",
        "clean_check" => "git clean check",
        "log" => "git log",
        "branch_list" => "git branch",
        "remote_status" => "git remote tracking status",
        "remote_list" => "git remote -v",
        _ => crate::cm_names::GIT,
    }
}

#[cfg(test)]
mod git_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::GitArgs;
    use serde_json::json;

    #[test]
    fn diff_keeps_flags_scope_and_path() {
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "diff"})).as_deref(),
            Some("git diff (working)")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "diff", "stat": true, "base": "main"})).as_deref(),
            Some("git diff --stat (main...HEAD)")
        );
        // `--stat` 优先于 `--name-only`（旧实现是 if / else if）。
        assert_eq!(
            GitArgs::summary_line(&json!({
                "action": "diff",
                "stat": true,
                "name_only": true,
                "path": " src/a.rs "
            }))
            .as_deref(),
            Some("git diff --stat (working): src/a.rs")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "diff", "name_only": true})).as_deref(),
            Some("git diff --name-only (working)")
        );
    }

    /// `mode` **不 trim**（旧实现直接 `unwrap_or("working")` 后 to_string）。
    #[test]
    fn diff_mode_is_not_trimmed() {
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "diff", "mode": " staged "})).as_deref(),
            Some("git diff ( staged )")
        );
    }

    #[test]
    fn show_and_path_actions() {
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "show"})).as_deref(),
            Some("git show HEAD")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "show", "rev": " HEAD~2 "})).as_deref(),
            Some("git show HEAD~2")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "blame"})).as_deref(),
            Some("git blame")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "file_history", "path": "a.rs"})).as_deref(),
            Some("git file history a.rs")
        );
    }

    #[test]
    fn read_only_actions_use_static_labels() {
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "status"})).as_deref(),
            Some("git status")
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "remote_list"})).as_deref(),
            Some("git remote -v")
        );
        // 缺 action / 拼错的 action 都回落工具名，不把原始字符串当标题。
        assert_eq!(
            GitArgs::summary_line(&json!({})).as_deref(),
            Some(crate::cm_names::GIT)
        );
        assert_eq!(
            GitArgs::summary_line(&json!({"action": "nope"})).as_deref(),
            Some(crate::cm_names::GIT)
        );
    }

    /// 与当前 action 无关的字段类型不符，同样不给摘要：旧结构体是**整体**反序列化。
    #[test]
    fn wrong_typed_field_drops_the_line() {
        assert!(GitArgs::summary_line(&json!({"action": "status", "stat": "yes"})).is_none());
        assert!(GitArgs::summary_line(&json!({"action": 7})).is_none());
        assert!(GitArgs::summary_line(&json!({"action": "diff", "path": [1]})).is_none());
    }
}
