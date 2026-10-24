//! 日程 / 提醒工具的 `action` 选择器分派层（2026-10-24 九合一）。
//!
//! 九个既有实现留在 [`super::schedule`]，这里只把 union 入参拆回各自的 per-action DTO：
//! 合并动的是对外 schema，不重写已验证的读写流程。代价是多一次 JSON 往返（参数体积很小）。
//!
//! 为什么单独成文件：`schedule.rs` 已接近 920 行的单文件上限，分派层与读写实现职责也不同。

use std::path::Path;

use crate::cm_tools::tools::tool_param_types::{
    EventArgs, EventAction, ReminderAction, ReminderArgs,
};

use super::schedule;

/// 提醒工具入口：`list` / `add` / `update` / `delete` / `complete`。
pub fn reminder(args_json: &str, working_dir: &Path) -> String {
    let args: ReminderArgs = match crate::cm_tools::tools::parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    match args.action {
        ReminderAction::List => schedule::list_reminders(
            &serde_json::json!({
                "include_done": args.include_done,
                "future_days": args.future_days,
            })
            .to_string(),
            working_dir,
        ),
        ReminderAction::Add => match non_empty(args.title.as_deref()) {
            Some(title) => schedule::add_reminder(
                &serde_json::json!({ "title": title, "due_at": args.due_at }).to_string(),
                working_dir,
            ),
            None => "参数错误：`add` 需要非空的 `title`。".to_string(),
        },
        ReminderAction::Update => match require_id(args.id.as_deref(), "reminder") {
            Ok(id) => schedule::update_reminder(
                &serde_json::json!({
                    "id": id,
                    "title": args.title,
                    "due_at": args.due_at,
                    "done": args.done,
                })
                .to_string(),
                working_dir,
            ),
            Err(e) => e,
        },
        ReminderAction::Delete => match require_id(args.id.as_deref(), "reminder") {
            Ok(id) => schedule::delete_reminder(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            ),
            Err(e) => e,
        },
        ReminderAction::Complete => match require_id(args.id.as_deref(), "reminder") {
            Ok(id) => schedule::complete_reminder(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            ),
            Err(e) => e,
        },
    }
}

/// 日程工具入口：`list` / `add` / `update` / `delete`。
pub fn event(args_json: &str, working_dir: &Path) -> String {
    let args: EventArgs = match crate::cm_tools::tools::parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    match args.action {
        EventAction::List => schedule::list_events(
            &serde_json::json!({
                "year": args.year,
                "month": args.month,
                "future_days": args.future_days,
            })
            .to_string(),
            working_dir,
        ),
        EventAction::Add => match (
            non_empty(args.title.as_deref()),
            non_empty(args.start_at.as_deref()),
        ) {
            (Some(title), Some(start_at)) => schedule::add_event(
                &serde_json::json!({
                    "title": title,
                    "start_at": start_at,
                    "end_at": args.end_at,
                    "location": args.location,
                    "notes": args.notes,
                })
                .to_string(),
                working_dir,
            ),
            _ => "参数错误：`add` 需要非空的 `title` 与 `start_at`。".to_string(),
        },
        EventAction::Update => match require_id(args.id.as_deref(), "event") {
            Ok(id) => schedule::update_event(
                &serde_json::json!({
                    "id": id,
                    "title": args.title,
                    "start_at": args.start_at,
                    "end_at": args.end_at,
                    "location": args.location,
                    "notes": args.notes,
                })
                .to_string(),
                working_dir,
            ),
            Err(e) => e,
        },
        EventAction::Delete => match require_id(args.id.as_deref(), "event") {
            Ok(id) => schedule::delete_event(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            ),
            Err(e) => e,
        },
    }
}

/// `trim` 后非空则返回（用于必填字符串字段）。
fn non_empty(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `id` 必填校验：缺失/空白时给出可操作的提示。
fn require_id(raw: Option<&str>, tool: &str) -> Result<String, String> {
    non_empty(raw).ok_or_else(|| {
        format!("参数错误：`id` 不能为空；可先用 `{tool}`（`action=list`）查看可用 id。")
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "crabmate_schedule_dispatch_{}_{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn cleanup_dir(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reminder_action_dispatch_covers_all_five() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        let added = super::reminder(r#"{"action":"add","title":"t1"}"#, ws);
        assert!(!added.contains("错误"), "{added}");
        let listed = super::reminder(r#"{"action":"list"}"#, ws);
        assert!(!listed.contains("错误"), "{listed}");
        // 其余三个 action 缺 `id` 时必须给出可操作提示 —— 也证明分派确实进到了各自分支。
        for action in ["update", "delete", "complete"] {
            let out = super::reminder(&format!(r#"{{"action":"{action}"}}"#), ws);
            assert!(out.contains("id"), "{action}: {out}");
        }
        let no_title = super::reminder(r#"{"action":"add"}"#, ws);
        assert!(no_title.contains("title"), "{no_title}");
        cleanup_dir(&dir);
    }

    #[test]
    fn event_action_dispatch_and_required_fields() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        let no_start = super::event(r#"{"action":"add","title":"e1"}"#, ws);
        assert!(no_start.contains("start_at"), "{no_start}");
        let added = super::event(
            r#"{"action":"add","title":"e1","start_at":"2030-01-02 10:00"}"#,
            ws,
        );
        assert!(!added.contains("错误"), "{added}");
        let listed = super::event(r#"{"action":"list","month":1}"#, ws);
        assert!(!listed.contains("错误"), "{listed}");
        for action in ["update", "delete"] {
            let out = super::event(&format!(r#"{{"action":"{action}"}}"#), ws);
            assert!(out.contains("id"), "{action}: {out}");
        }
        cleanup_dir(&dir);
    }

    #[test]
    fn unknown_action_and_missing_action_are_rejected() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        assert!(super::reminder(r#"{"action":"nope"}"#, ws).contains("参数解析错误"));
        assert!(super::reminder(r#"{}"#, ws).contains("参数解析错误"));
        assert!(super::event(r#"{"action":"complete"}"#, ws).contains("参数解析错误"));
        cleanup_dir(&dir);
    }
}
