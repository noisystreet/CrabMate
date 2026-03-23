//! 工作区列表、任务/日程、assistant 占位更新。

use crate::types::Message;

use super::state::{Mode, TuiState};

const FILE_VIEW_PREVIEW_MAX_CHARS: usize = 800_000;

/// 流式增量写入**当前轮**助手气泡。
///
/// 必须只更新**列表末尾**的助手：若从尾部向前找「任意一条」助手并改写，会在分阶段规划下把**规划轮**正文覆盖成分步执行的流式输出（表现为「队列里已有步骤，上一轮模型气泡消失」）；工具链末尾为 `tool` 时也应**新推**助手而非改写更早的 `tool_calls` 那条。
///
/// 分步 `user` 已插入后，若滞后 SSE 仍送来**与上一条助手全文相同**的正文，不得再 `push` 第二条助手（否则与 `sync_merge` 去重前即出现相邻重复规划气泡）。
pub(super) fn upsert_assistant_message(messages: &mut Vec<Message>, content: &str) {
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
    {
        last.content = Some(content.to_string());
        return;
    }
    if messages.len() >= 2
        && messages.last().is_some_and(|m| m.role == "user")
        && let Some(prev) = messages.get(messages.len() - 2)
        && prev.role == "assistant"
        && prev.tool_calls.is_none()
        && prev.content.as_deref() == Some(content)
    {
        return;
    }
    messages.push(Message {
        role: "assistant".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    });
}

pub(super) fn refresh_workspace(state: &mut TuiState) {
    let mut entries = Vec::new();
    let dir = &state.workspace_dir;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
            entries.push((name, is_dir));
        }
        entries.sort_by(|a, b| match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        });
    }
    state.workspace_entries = entries;
    state.workspace_sel = state
        .workspace_sel
        .min(state.workspace_entries.len().saturating_sub(1));
}

pub(super) fn refresh_tasks(state: &mut TuiState) {
    let path = state.workspace_dir.join("tasks.json");
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            state.task_items = Vec::new();
            state.task_sel = 0;
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for it in items {
        let id = it
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let title = it
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let done = it.get("done").and_then(|x| x.as_bool()).unwrap_or(false);
        if !title.is_empty() {
            out.push((id, title, done));
        }
    }
    state.task_items = out;
    state.task_sel = state.task_sel.min(state.task_items.len().saturating_sub(1));
}

pub(super) fn refresh_schedule(state: &mut TuiState) {
    let rpath = state.workspace_dir.join(".crabmate").join("reminders.json");
    let mut reminders = Vec::new();
    if let Ok(s) = std::fs::read_to_string(&rpath) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr.iter().take(200) {
                let id = it
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = it
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let done = it.get("done").and_then(|x| x.as_bool()).unwrap_or(false);
                let due_at = it
                    .get("due_at")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                if !title.is_empty() {
                    reminders.push((id, title, done, due_at));
                }
            }
        }
    }
    state.reminder_items = reminders;
    state.reminder_sel = state
        .reminder_sel
        .min(state.reminder_items.len().saturating_sub(1));

    let epath = state.workspace_dir.join(".crabmate").join("events.json");
    let mut events = Vec::new();
    if let Ok(s) = std::fs::read_to_string(&epath) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr.iter().take(200) {
                let id = it
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = it
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let start = it
                    .get("start_at")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if !title.is_empty() {
                    events.push((id, title, start));
                }
            }
        }
    }
    state.event_items = events;
    state.event_sel = state
        .event_sel
        .min(state.event_items.len().saturating_sub(1));
}

pub(super) fn split_title_due(s: &str) -> (String, Option<String>) {
    if let Some((a, b)) = s.split_once('@') {
        let title = a.trim().to_string();
        let due = b.trim().to_string();
        if due.is_empty() {
            (title, None)
        } else {
            (title, Some(due))
        }
    } else {
        (s.trim().to_string(), None)
    }
}

pub(super) fn workspace_go_up(state: &mut TuiState) {
    if let Some(p) = state.workspace_dir.parent() {
        state.workspace_dir = p.to_path_buf();
        refresh_workspace(state);
        refresh_tasks(state);
        refresh_schedule(state);
    }
}

pub(super) fn workspace_open_or_enter(state: &mut TuiState) {
    let Some((name, is_dir)) = state.workspace_entries.get(state.workspace_sel).cloned() else {
        return;
    };
    let path = state.workspace_dir.join(&name);
    if is_dir {
        state.workspace_dir = path;
        refresh_workspace(state);
        refresh_tasks(state);
        refresh_schedule(state);
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| format!("读取失败：{}", e));
    let content = if content.len() > FILE_VIEW_PREVIEW_MAX_CHARS {
        let mut end = FILE_VIEW_PREVIEW_MAX_CHARS;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n\n...(内容过长已截断：预览前 {} 字符，共 {} 字符)",
            &content[..end],
            end,
            content.chars().count()
        )
    } else {
        content
    };
    state.file_view_title = path.display().to_string();
    state.file_view_content = content;
    state.mode = Mode::FileView;
}

pub(super) fn toggle_task_done(state: &mut TuiState) {
    if state.task_items.is_empty() {
        return;
    }
    let idx = state.task_sel.min(state.task_items.len() - 1);
    state.task_items[idx].2 = !state.task_items[idx].2;
    let path = state.workspace_dir.join("tasks.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let items: Vec<serde_json::Value> = state
        .task_items
        .iter()
        .map(|(id, title, done)| serde_json::json!({ "id": id, "title": title, "done": done }))
        .collect();
    root["items"] = serde_json::Value::Array(items);
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        if let Err(e) = std::fs::write(&path, s.as_bytes()) {
            state.status_line = format!("写入 tasks.json 失败：{}", e);
        } else {
            state.status_line = "任务状态已更新".to_string();
        }
    } else {
        state.status_line = "序列化 tasks.json 失败".to_string();
    }
}

pub(super) fn toggle_reminder_done(state: &mut TuiState) {
    if state.reminder_items.is_empty() {
        return;
    }
    let idx = state.reminder_sel.min(state.reminder_items.len() - 1);
    state.reminder_items[idx].2 = !state.reminder_items[idx].2;
    let path = state.workspace_dir.join(".crabmate").join("reminders.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let items: Vec<serde_json::Value> = state
        .reminder_items
        .iter()
        .map(|(id, title, done, due_at)| {
            let mut v = serde_json::json!({ "id": id, "title": title, "done": done });
            if let Some(d) = due_at {
                v["due_at"] = serde_json::Value::String(d.clone());
            }
            v
        })
        .collect();
    root["items"] = serde_json::Value::Array(items);
    if let Err(e) = std::fs::create_dir_all(state.workspace_dir.join(".crabmate")) {
        state.status_line = format!("创建 .crabmate 目录失败：{}", e);
        return;
    }
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        if let Err(e) = std::fs::write(&path, s.as_bytes()) {
            state.status_line = format!("写入 reminders.json 失败：{}", e);
        } else {
            state.status_line = "提醒状态已更新".to_string();
        }
    } else {
        state.status_line = "序列化 reminders.json 失败".to_string();
    }
}

#[cfg(test)]
mod upsert_assistant_tests {
    use super::upsert_assistant_message;
    use crate::types::Message;

    fn a(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn upsert_updates_only_trailing_assistant() {
        let mut m = vec![
            Message::user_only("u1"),
            a("planning"),
            Message::user_only("step"),
        ];
        upsert_assistant_message(&mut m, "step reply");
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].content.as_deref(), Some("planning"));
        assert_eq!(m[3].content.as_deref(), Some("step reply"));
    }

    #[test]
    fn upsert_after_tool_pushes_new_assistant() {
        let tool = Message {
            role: "tool".to_string(),
            content: Some("{}".to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: Some("c1".to_string()),
        };
        let mut m = vec![Message::user_only("q"), a("call"), tool];
        upsert_assistant_message(&mut m, "after tool");
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].content.as_deref(), Some("call"));
        assert_eq!(m[3].content.as_deref(), Some("after tool"));
    }

    #[test]
    fn upsert_last_assistant_in_place() {
        let mut m = vec![Message::user_only("q"), a("x")];
        upsert_assistant_message(&mut m, "xy");
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content.as_deref(), Some("xy"));
    }

    #[test]
    fn upsert_ignores_stale_duplicate_plan_after_step_user() {
        let plan = "full plan body identical";
        let mut m = vec![Message::user_only("q"), a(plan), Message::user_only("step")];
        upsert_assistant_message(&mut m, plan);
        assert_eq!(m.len(), 3, "不得再推一条与规划全文相同的助手");
        assert_eq!(m[1].content.as_deref(), Some(plan));
    }
}
