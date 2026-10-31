// ── gh *（`gh_pr` / `gh_run`）──────────────────────────────────

/// `repo` 后缀：只有 trim 后非空时才追加 ` (owner/repo)`。
fn gh_repo_suffix(repo: Option<&str>) -> String {
    repo.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({r})"))
        .unwrap_or_default()
}

/// `gh_pr` 的摘要行：`action` 必填（旧 `GhPrSummaryArgs` 里它没有 `#[serde(default)]`），
/// 其余字段**先全部取值**再按 action 拼装——旧结构体是整体反序列化，用不到的字段类型不符
/// 同样不该有摘要。
impl ToolArgsSummary for super::tool_param_types::GhPrArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action = arg_str_raw(args, "action")?.trim();
        let repo = arg_opt_str(args, "repo")?;
        let number = arg_opt_u64(args, "number")?;
        let title = arg_opt_str(args, "title")?;
        let merge_method = arg_opt_str(args, "merge_method")?;
        let event = arg_opt_str(args, "event")?;
        let draft = arg_opt_bool(args, "draft")?;
        if action.is_empty() {
            return None;
        }
        let mut line = format!("gh pr {action}");
        // PR 号只在 > 0 时显示（旧实现 `.filter(|n| *n > 0)`）。
        if let Some(n) = number.filter(|n| *n > 0) {
            line.push_str(&format!(" #{n}"));
        }
        line.push_str(&gh_pr_action_suffix(action, merge_method, event, draft));
        line.push_str(&gh_repo_suffix(repo));
        line.push_str(&gh_pr_title_suffix(title));
        Some(line)
    }
}

/// `merge` 的 `(method)`、`review` 的 event、`create` 的 ` draft`；其它 action 无附加。
fn gh_pr_action_suffix(
    action: &str,
    merge_method: Option<&str>,
    event: Option<&str>,
    draft: Option<bool>,
) -> String {
    match action {
        // `merge_method` **不 trim**（缺省 `rebase`）。
        "merge" => format!(" ({})", merge_method.unwrap_or("rebase")),
        "review" => match event.unwrap_or("").trim() {
            "" => String::new(),
            ev => format!(" {ev}"),
        },
        "create" if draft == Some(true) => " draft".to_string(),
        _ => String::new(),
    }
}

/// 标题只取前 40 个字符、超出补省略号；空标题（多数 action 没这个字段）不追加。
fn gh_pr_title_suffix(title: Option<&str>) -> String {
    let t = title.unwrap_or("").trim();
    if t.is_empty() {
        return String::new();
    }
    let mut head: String = t.chars().take(40).collect();
    if t.chars().count() > 40 {
        head.push('\u{2026}');
    }
    format!(": {head}")
}

/// `gh_run` 的摘要行：`action` 与 `run_id` 都必填（旧结构体两者都没有 `#[serde(default)]`）。
impl ToolArgsSummary for super::tool_param_types::GhRunArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let action = arg_str_raw(args, "action")?.trim();
        let run_id = arg_str_raw(args, "run_id")?.trim();
        let failed = arg_opt_bool(args, "failed")?;
        if run_id.is_empty() {
            return None;
        }
        Some(match action {
            "rerun" if failed == Some(true) => format!("gh run rerun {run_id} --failed"),
            "rerun" => format!("gh run rerun {run_id}"),
            "failure_summary" => format!("gh run failure summary {run_id}"),
            other => format!("gh run {other} {run_id}"),
        })
    }
}

#[cfg(test)]
mod gh_pr_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::GhPrArgs;
    use serde_json::json;

    #[test]
    fn merge_shows_method_number_and_repo() {
        assert_eq!(
            GhPrArgs::summary_line(&json!({
                "action": "merge",
                "number": 12,
                "repo": " o/r ",
                "merge_method": "squash"
            }))
            .as_deref(),
            Some("gh pr merge #12 (squash) (o/r)")
        );
        // `merge_method` 缺省 rebase，且**不 trim**。
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "merge"})).as_deref(),
            Some("gh pr merge (rebase)")
        );
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "merge", "merge_method": " squash "}))
                .as_deref(),
            Some("gh pr merge ( squash )")
        );
    }

    #[test]
    fn review_event_and_draft_are_appended() {
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "review", "event": " approve "})).as_deref(),
            Some("gh pr review approve")
        );
        // 空 event 不追加尾部空格。
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "review", "event": "  "})).as_deref(),
            Some("gh pr review")
        );
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "create", "draft": true})).as_deref(),
            Some("gh pr create draft")
        );
    }

    /// 标题取前 40 字符并补省略号；`number: 0` 不显示 `#0`。
    #[test]
    fn title_truncates_and_zero_number_is_hidden() {
        let title = "一".repeat(45);
        let got = GhPrArgs::summary_line(&json!({
            "action": "create",
            "number": 0,
            "title": title
        }))
        .unwrap();
        assert_eq!(got, format!("gh pr create: {}\u{2026}", "一".repeat(40)));
        assert_eq!(
            GhPrArgs::summary_line(&json!({"action": "checks", "title": "  "})).as_deref(),
            Some("gh pr checks")
        );
    }

    /// `action` 必填；用不到的字段类型不符同样不给摘要（旧结构体整体反序列化）。
    #[test]
    fn missing_or_badly_typed_fields_drop_the_line() {
        assert!(GhPrArgs::summary_line(&json!({"number": 3})).is_none());
        assert!(GhPrArgs::summary_line(&json!({"action": "   "})).is_none());
        assert!(GhPrArgs::summary_line(&json!({"action": "checks", "draft": "yes"})).is_none());
        assert!(GhPrArgs::summary_line(&json!({"action": "checks", "number": -1})).is_none());
    }
}

#[cfg(test)]
mod gh_run_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::GhRunArgs;
    use serde_json::json;

    #[test]
    fn rerun_and_failure_summary() {
        assert_eq!(
            GhRunArgs::summary_line(&json!({"action": "rerun", "run_id": " 42 "})).as_deref(),
            Some("gh run rerun 42")
        );
        assert!(
            GhRunArgs::summary_line(&json!({"action": "rerun", "run_id": 42, "failed": true}))
                .is_none(),
            "run_id 是 String，数字不合法"
        );
        assert_eq!(
            GhRunArgs::summary_line(&json!({"action": "rerun", "run_id": "42", "failed": true}))
                .as_deref(),
            Some("gh run rerun 42 --failed")
        );
        assert_eq!(
            GhRunArgs::summary_line(&json!({"action": "failure_summary", "run_id": "42"}))
                .as_deref(),
            Some("gh run failure summary 42")
        );
    }

    #[test]
    fn unknown_action_keeps_the_raw_name() {
        assert_eq!(
            GhRunArgs::summary_line(&json!({"action": "nope", "run_id": "42"})).as_deref(),
            Some("gh run nope 42")
        );
    }

    #[test]
    fn required_fields_and_types_are_enforced() {
        assert!(GhRunArgs::summary_line(&json!({"action": "rerun"})).is_none());
        assert!(GhRunArgs::summary_line(&json!({"run_id": "42"})).is_none());
        assert!(GhRunArgs::summary_line(&json!({"action": "rerun", "run_id": "  "})).is_none());
        assert!(
            GhRunArgs::summary_line(&json!({"action": "rerun", "run_id": "42", "failed": "yes"}))
                .is_none()
        );
    }
}
