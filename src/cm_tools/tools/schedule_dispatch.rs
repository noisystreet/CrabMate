//! 日程 / 提醒工具的 `action` 选择器分派层（2026-10-24 九合一）。
//!
//! 九个既有实现留在 [`super::schedule`]，这里只把 union 入参拆回各自的 per-action DTO：
//! 合并动的是对外 schema，不重写已验证的读写流程。代价是多一次 JSON 往返（参数体积很小）。
//!
//! 为什么单独成文件：`schedule.rs` 已接近 920 行的单文件上限，分派层与读写实现职责也不同。

use std::path::Path;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::{
    EventArgs, EventAction, ReminderAction, ReminderArgs,
};

use super::schedule;

/// 提醒工具入口：`list` / `add` / `update` / `delete` / `complete`。
///
/// `add` 缺 `title` 与 `update` / `delete` / `complete` 缺 `id` 在此收敛为 `invalid_args`：
/// 这两类分支旧版只返回一段既无「错误」前缀也无「失败」关键词的正文，
/// 会被 `parse_tool_output` 判成**成功**（模型传错参数却拿到「成功」）。
#[allow(clippy::result_large_err)]
pub fn reminder(args_json: &str, working_dir: &Path) -> Result<String, ToolError> {
    let args: ReminderArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    match args.action {
        ReminderAction::List => schedule::list_reminders(
            &serde_json::json!({
                "include_done": args.include_done,
                "future_days": args.future_days,
            })
            .to_string(),
            working_dir,
        ),
        ReminderAction::Add => {
            let title = non_empty(args.title.as_deref()).ok_or_else(|| {
                ToolError::invalid_args("参数错误：`add` 需要非空的 `title`。".to_string())
            })?;
            schedule::add_reminder(
                &serde_json::json!({ "title": title, "due_at": args.due_at }).to_string(),
                working_dir,
            )
        }
        ReminderAction::Update => {
            let id = require_id(args.id.as_deref(), "reminder")?;
            schedule::update_reminder(
                &serde_json::json!({
                    "id": id,
                    "title": args.title,
                    "due_at": args.due_at,
                    "done": args.done,
                })
                .to_string(),
                working_dir,
            )
        }
        ReminderAction::Delete => {
            let id = require_id(args.id.as_deref(), "reminder")?;
            schedule::delete_reminder(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            )
        }
        ReminderAction::Complete => {
            let id = require_id(args.id.as_deref(), "reminder")?;
            schedule::complete_reminder(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            )
        }
    }
}

/// 日程工具入口：`list` / `add` / `update` / `delete`。
///
/// 与 [`reminder`] 同：`add` 的必填字段与 `update` / `delete` 的 `id` 在这里收敛为
/// `invalid_args`，不再返回会被判成「成功」的裸正文。
#[allow(clippy::result_large_err)]
pub fn event(args_json: &str, working_dir: &Path) -> Result<String, ToolError> {
    let args: EventArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
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
        EventAction::Add => {
            let (Some(title), Some(start_at)) = (
                non_empty(args.title.as_deref()),
                non_empty(args.start_at.as_deref()),
            ) else {
                return Err(ToolError::invalid_args(
                    "参数错误：`add` 需要非空的 `title` 与 `start_at`。".to_string(),
                ));
            };
            schedule::add_event(
                &serde_json::json!({
                    "title": title,
                    "start_at": start_at,
                    "end_at": args.end_at,
                    "location": args.location,
                    "notes": args.notes,
                })
                .to_string(),
                working_dir,
            )
        }
        EventAction::Update => {
            let id = require_id(args.id.as_deref(), "event")?;
            schedule::update_event(
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
            )
        }
        EventAction::Delete => {
            let id = require_id(args.id.as_deref(), "event")?;
            schedule::delete_event(
                &serde_json::json!({ "id": id }).to_string(),
                working_dir,
            )
        }
    }
}

/// `trim` 后非空则返回（用于必填字符串字段）。
fn non_empty(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `id` 必填校验：缺失/空白时给出可操作的提示（`invalid_args`）。
#[allow(clippy::result_large_err)]
fn require_id(raw: Option<&str>, tool: &str) -> Result<String, ToolError> {
    non_empty(raw).ok_or_else(|| {
        ToolError::invalid_args(format!(
            "参数错误：`id` 不能为空；可先用 `{tool}`（`action=list`）查看可用 id。"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::cm_tools::tool_result::ToolError;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 期望成功；失败时把 `error_code` 与正文一并写进 panic，便于定位。
    fn ok(r: Result<String, ToolError>) -> String {
        match r {
            Ok(s) => s,
            Err(e) => panic!("期望成功，实际失败[{}]: {}", e.code, e.message),
        }
    }

    /// 期望失败，返回 `error_code`；成功即 panic —— 静默成功正是本文件要防的缺陷。
    fn err_code(r: Result<String, ToolError>) -> String {
        match r {
            Ok(s) => panic!("期望失败，实际成功: {s}"),
            Err(e) => e.code,
        }
    }

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

    /// 读工作区日程文件的 `items[0].id`。
    ///
    /// 刻意不解析成功文案（`…（id=xxx）`）：断言只关心行为，文案改动不该连累测试。
    fn first_id(ws: &Path, file: &str) -> String {
        let raw = std::fs::read_to_string(ws.join(".crabmate").join(file)).expect("日程 JSON 文件");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("日程 JSON");
        v["items"][0]["id"]
            .as_str()
            .expect("`items[0].id` 应为字符串")
            .to_string()
    }

    #[test]
    fn reminder_dispatch_runs_every_writing_action() {
        let dir = make_temp_dir();
        let ws = dir.as_path();

        let added = ok(super::reminder(
            r#"{"action":"add","title":"买牛奶","due_at":"2030-01-02"}"#,
            ws,
        ));
        assert!(added.contains("已添加提醒"), "{added}");
        let id = first_id(ws, "reminders.json");

        // `complete`：默认列表随即隐藏，`include_done` 才可见 —— 两条路径都走一遍。
        let done = ok(super::reminder(
            &format!(r#"{{"action":"complete","id":"{id}"}}"#),
            ws,
        ));
        assert!(done.contains("已完成提醒"), "{done}");
        let visible = ok(super::reminder(r#"{"action":"list"}"#, ws));
        assert!(!visible.contains("买牛奶"), "{visible}");
        let with_done = ok(super::reminder(
            r#"{"action":"list","include_done":true}"#,
            ws,
        ));
        assert!(with_done.contains("买牛奶"), "{with_done}");

        // `update`：改标题 + 空串清空 `due_at`（旧实现的既有约定）。
        let updated = ok(super::reminder(
            &format!(r#"{{"action":"update","id":"{id}","title":"买燕麦奶","due_at":""}}"#),
            ws,
        ));
        assert!(updated.contains("已更新提醒"), "{updated}");
        let renamed = ok(super::reminder(
            r#"{"action":"list","include_done":true}"#,
            ws,
        ));
        assert!(renamed.contains("买燕麦奶"), "{renamed}");

        // `delete`：文件里不应再剩任何条目。
        let deleted = ok(super::reminder(
            &format!(r#"{{"action":"delete","id":"{id}"}}"#),
            ws,
        ));
        assert!(deleted.contains("已删除提醒"), "{deleted}");
        let after = ok(super::reminder(
            r#"{"action":"list","include_done":true}"#,
            ws,
        ));
        assert!(!after.contains("买燕麦奶"), "{after}");

        cleanup_dir(&dir);
    }

    #[test]
    fn event_dispatch_runs_every_writing_action() {
        let dir = make_temp_dir();
        let ws = dir.as_path();

        // RFC3339 起点：`list --year` 过滤依赖它可解析。
        let added = ok(super::event(
            r#"{"action":"add","title":"体检","start_at":"2030-01-02T10:00:00Z","location":"医院"}"#,
            ws,
        ));
        assert!(added.contains("已添加日程"), "{added}");
        let id = first_id(ws, "events.json");

        // `update`：改标题 + 空串清空 `location`。
        let updated = ok(super::event(
            &format!(r#"{{"action":"update","id":"{id}","title":"复检","location":""}}"#),
            ws,
        ));
        assert!(updated.contains("已更新日程"), "{updated}");
        let renamed = ok(super::event(r#"{"action":"list","year":2030}"#, ws));
        assert!(renamed.contains("复检"), "{renamed}");
        assert!(!renamed.contains("医院"), "{renamed}");

        // `delete`
        let deleted = ok(super::event(
            &format!(r#"{{"action":"delete","id":"{id}"}}"#),
            ws,
        ));
        assert!(deleted.contains("已删除日程"), "{deleted}");
        let after = ok(super::event(r#"{"action":"list","year":2030}"#, ws));
        assert!(!after.contains("复检"), "{after}");

        cleanup_dir(&dir);
    }

    #[test]
    fn reminder_requires_title_and_id() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        // 回归：这两类必填校验旧版只返回裸正文（无「错误」前缀、无「失败」关键词），被判成功。
        let no_title = super::reminder(r#"{"action":"add"}"#, ws);
        assert_eq!(err_code(no_title), "invalid_args");
        let blank_title = super::reminder(r#"{"action":"add","title":"   "}"#, ws);
        assert_eq!(err_code(blank_title), "invalid_args");
        for action in ["update", "delete", "complete"] {
            let out = super::reminder(&format!(r#"{{"action":"{action}"}}"#), ws);
            assert_eq!(err_code(out), "invalid_args", "{action}");
        }
        cleanup_dir(&dir);
    }

    #[test]
    fn event_requires_title_start_at_and_id() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        let no_start = super::event(r#"{"action":"add","title":"e1"}"#, ws);
        assert_eq!(err_code(no_start), "invalid_args");
        let blank_start = super::event(r#"{"action":"add","title":"e1","start_at":""}"#, ws);
        assert_eq!(err_code(blank_start), "invalid_args");
        for action in ["update", "delete"] {
            let out = super::event(&format!(r#"{{"action":"{action}"}}"#), ws);
            assert_eq!(err_code(out), "invalid_args", "{action}");
        }
        cleanup_dir(&dir);
    }

    #[test]
    fn unknown_action_and_missing_action_are_rejected() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        let bad_action = super::reminder(r#"{"action":"nope"}"#, ws);
        assert_eq!(err_code(bad_action), "invalid_args");
        let missing = super::reminder(r#"{}"#, ws);
        assert_eq!(err_code(missing), "invalid_args");
        // `complete` 是 reminder 的 action，不属于 event。
        let wrong_family = super::event(r#"{"action":"complete"}"#, ws);
        assert_eq!(err_code(wrong_family), "invalid_args");
        cleanup_dir(&dir);
    }

    #[test]
    fn unknown_id_reports_not_found_instead_of_success() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        // 回归：`未找到提醒：id=…` 不含「错误 / 失败 / 超时」关键词，旧版被判成功。
        for action in ["complete", "delete"] {
            let out = super::reminder(
                &format!(r#"{{"action":"{action}","id":"rem_missing"}}"#),
                ws,
            );
            assert_eq!(err_code(out), "reminder_not_found", "{action}");
        }
        let updated = super::reminder(
            r#"{"action":"update","id":"rem_missing","title":"x"}"#,
            ws,
        );
        assert_eq!(err_code(updated), "reminder_not_found");

        for action in ["update", "delete"] {
            let out = super::event(
                &format!(r#"{{"action":"{action}","id":"evt_missing","title":"x"}}"#),
                ws,
            );
            assert_eq!(err_code(out), "event_not_found", "{action}");
        }
        cleanup_dir(&dir);
    }

    #[test]
    fn empty_list_is_success_not_failure() {
        let dir = make_temp_dir();
        let ws = dir.as_path();
        // 「列表为空」是合法答案，不是失败 —— 迁移不得把它变成 Err。
        assert!(ok(super::reminder(r#"{"action":"list"}"#, ws)).contains("提醒列表为空"));
        assert!(ok(super::event(r#"{"action":"list"}"#, ws)).contains("日程列表为空"));
        cleanup_dir(&dir);
    }
}
