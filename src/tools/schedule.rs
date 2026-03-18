//! 日程与提醒工具：在工作区内以 JSON 文件形式持久化。
//!
//! 文件位置：<working_dir>/.crabmate/reminders.json 与 <working_dir>/.crabmate/events.json

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DATA_DIR: &str = ".crabmate";
const REMINDERS_FILE: &str = "reminders.json";
const EVENTS_FILE: &str = "events.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Reminder {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    due_at: Option<String>,
    done: bool,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Event {
    id: String,
    title: String,
    start_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct RemindersData {
    items: Vec<Reminder>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct EventsData {
    items: Vec<Event>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn gen_id(prefix: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}_{}_{}", prefix, std::process::id(), ts)
}

fn data_dir(root: &Path) -> PathBuf {
    root.join(DATA_DIR)
}

fn reminders_path(root: &Path) -> PathBuf {
    data_dir(root).join(REMINDERS_FILE)
}

fn events_path(root: &Path) -> PathBuf {
    data_dir(root).join(EVENTS_FILE)
}

fn ensure_data_dir(root: &Path) -> Result<(), String> {
    let dir = data_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败：{}（{}）", dir.display(), e))
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    let s = std::fs::read_to_string(path).map_err(|e| format!("读取失败：{}（{}）", path.display(), e))?;
    serde_json::from_str(&s).map_err(|e| format!("解析失败：{}（{}）", path.display(), e))
}

fn write_json<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(data).map_err(|e| format!("序列化失败：{}", e))?;
    std::fs::write(path, s.as_bytes()).map_err(|e| format!("写入失败：{}（{}）", path.display(), e))
}

fn parse_datetime_to_rfc3339(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // RFC3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).to_rfc3339());
    }
    // "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" (local time)
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
    {
        let local_dt = Local.from_local_datetime(&ndt).single()?;
        return Some(local_dt.with_timezone(&Utc).to_rfc3339());
    }
    // "YYYY-MM-DD" (treat as 00:00 local)
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = d.and_hms_opt(0, 0, 0)?;
        let local_dt = Local.from_local_datetime(&ndt).single()?;
        return Some(local_dt.with_timezone(&Utc).to_rfc3339());
    }
    None
}

fn format_dt_display(rfc3339: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(rfc3339) {
        return dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();
    }
    rfc3339.to_string()
}

fn parse_rfc3339_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ---------------- Reminders ----------------

pub fn add_reminder(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let title = match v.get("title").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 title 参数".to_string(),
    };
    let due_at_raw = v.get("due_at").and_then(|x| x.as_str()).unwrap_or("").trim();
    let due_at = if due_at_raw.is_empty() {
        None
    } else {
        parse_datetime_to_rfc3339(due_at_raw).or_else(|| Some(due_at_raw.to_string()))
    };

    if let Err(e) = ensure_data_dir(working_dir) {
        return e;
    }
    let path = reminders_path(working_dir);
    let mut data: RemindersData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let now = now_rfc3339();
    let item = Reminder {
        id: gen_id("rem"),
        title,
        due_at,
        done: false,
        created_at: now,
        updated_at: None,
    };
    let id = item.id.clone();
    data.items.push(item);
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已添加提醒：{}（id={}）", data.items.last().unwrap().title, id)
}

pub fn list_reminders(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = serde_json::from_str(args_json).unwrap_or_else(|_| serde_json::json!({}));
    let include_done = v.get("include_done").and_then(|b| b.as_bool()).unwrap_or(false);
    let future_days = v.get("future_days").and_then(|d| d.as_u64()).map(|d| d as i64);

    let path = reminders_path(working_dir);
    let mut data: RemindersData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };

    // 按到期时间排序（无到期时间排后）
    data.items.sort_by(|a, b| {
        let ad = a.due_at.as_deref().and_then(parse_rfc3339_utc);
        let bd = b.due_at.as_deref().and_then(parse_rfc3339_utc);
        match (ad, bd) {
            (Some(ad), Some(bd)) => ad.cmp(&bd),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.created_at.cmp(&b.created_at),
        }
    });

    let now = Utc::now();
    let end = future_days.map(|d| now + chrono::Duration::days(d.max(0)));
    let items: Vec<&Reminder> = data
        .items
        .iter()
        .filter(|r| include_done || !r.done)
        .filter(|r| {
            let Some(end) = end else { return true; };
            let Some(due) = r.due_at.as_deref().and_then(parse_rfc3339_utc) else { return false; };
            due >= now && due <= end
        })
        .collect();
    if items.is_empty() {
        return "提醒列表为空。".to_string();
    }
    let mut out = String::new();
    out.push_str(&format!("提醒（{} 条）：\n", items.len()));
    for r in items {
        let status = if r.done { "已完成" } else { "待办" };
        let due = r
            .due_at
            .as_deref()
            .map(format_dt_display)
            .map(|s| format!("，到期：{}", s))
            .unwrap_or_default();
        out.push_str(&format!("- [{}] {}（id={}{}）\n", status, r.title, r.id, due));
    }
    out.trim_end().to_string()
}

pub fn complete_reminder(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let id = match v.get("id").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 id 参数".to_string(),
    };
    let path = reminders_path(working_dir);
    let mut data: RemindersData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut found = None;
    for r in &mut data.items {
        if r.id == id {
            r.done = true;
            r.updated_at = Some(now_rfc3339());
            found = Some(r.title.clone());
            break;
        }
    }
    let title = match found {
        Some(t) => t,
        None => return format!("未找到提醒：id={}", id),
    };
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已完成提醒：{}（id={}）", title, id)
}

pub fn update_reminder(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let id = match v.get("id").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 id 参数".to_string(),
    };
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let due_at = v.get("due_at").and_then(|x| x.as_str()).map(|s| s.trim().to_string());
    let done = v.get("done").and_then(|b| b.as_bool());

    if title.is_none() && due_at.is_none() && done.is_none() {
        return "错误：至少提供 title/due_at/done 中的一个用于更新".to_string();
    }

    let path = reminders_path(working_dir);
    let mut data: RemindersData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut found = None;
    for r in &mut data.items {
        if r.id == id {
            if let Some(t) = title.clone() {
                r.title = t;
            }
            if let Some(d) = due_at.as_deref() {
                let d = d.trim();
                if d.is_empty() {
                    r.due_at = None;
                } else {
                    r.due_at = parse_datetime_to_rfc3339(d).or_else(|| Some(d.to_string()));
                }
            }
            if let Some(done) = done {
                r.done = done;
            }
            r.updated_at = Some(now_rfc3339());
            found = Some(r.title.clone());
            break;
        }
    }
    let title = match found {
        Some(t) => t,
        None => return format!("未找到提醒：id={}", id),
    };
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已更新提醒：{}（id={}）", title, id)
}

pub fn delete_reminder(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let id = match v.get("id").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 id 参数".to_string(),
    };
    let path = reminders_path(working_dir);
    let mut data: RemindersData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let before = data.items.len();
    data.items.retain(|r| r.id != id);
    if data.items.len() == before {
        return format!("未找到提醒：id={}", id);
    }
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已删除提醒：id={}", id)
}

// ---------------- Events ----------------

pub fn add_event(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let title = match v.get("title").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 title 参数".to_string(),
    };
    let start_at_raw = match v.get("start_at").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s,
        _ => return "错误：缺少 start_at 参数".to_string(),
    };
    let start_at = parse_datetime_to_rfc3339(start_at_raw).unwrap_or_else(|| start_at_raw.to_string());
    let end_at_raw = v.get("end_at").and_then(|x| x.as_str()).unwrap_or("").trim();
    let end_at = if end_at_raw.is_empty() {
        None
    } else {
        Some(parse_datetime_to_rfc3339(end_at_raw).unwrap_or_else(|| end_at_raw.to_string()))
    };
    let location = v
        .get("location")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let notes = v
        .get("notes")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Err(e) = ensure_data_dir(working_dir) {
        return e;
    }
    let path = events_path(working_dir);
    let mut data: EventsData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let now = now_rfc3339();
    let item = Event {
        id: gen_id("evt"),
        title,
        start_at,
        end_at,
        location,
        notes,
        created_at: now,
        updated_at: None,
    };
    let id = item.id.clone();
    data.items.push(item);
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已添加日程：{}（id={}）", data.items.last().unwrap().title, id)
}

pub fn list_events(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = serde_json::from_str(args_json).unwrap_or_else(|_| serde_json::json!({}));
    let year = v.get("year").and_then(|y| y.as_i64()).map(|y| y as i32);
    let month = v.get("month").and_then(|m| m.as_u64()).and_then(|m| u32::try_from(m).ok());
    let future_days = v.get("future_days").and_then(|d| d.as_u64()).map(|d| d as i64);

    let path = events_path(working_dir);
    let mut data: EventsData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };

    // 按开始时间排序（无法解析的排后）
    data.items.sort_by(|a, b| {
        let ad = parse_rfc3339_utc(&a.start_at);
        let bd = parse_rfc3339_utc(&b.start_at);
        match (ad, bd) {
            (Some(ad), Some(bd)) => ad.cmp(&bd),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.start_at.cmp(&b.start_at),
        }
    });

    let now = Utc::now();
    let end = future_days.map(|d| now + chrono::Duration::days(d.max(0)));
    let items: Vec<&Event> = data
        .items
        .iter()
        .filter(|e| match (year, month) {
            (None, None) => true,
            (Some(y), None) => e.start_at.starts_with(&format!("{:04}-", y)),
            (Some(y), Some(m)) => e.start_at.starts_with(&format!("{:04}-{:02}-", y, m)),
            (None, Some(_)) => true,
        })
        .filter(|e| {
            let Some(end) = end else { return true; };
            let Some(start) = parse_rfc3339_utc(&e.start_at) else { return false; };
            start >= now && start <= end
        })
        .collect();

    if items.is_empty() {
        return "日程列表为空。".to_string();
    }
    let mut out = String::new();
    out.push_str(&format!("日程（{} 条）：\n", items.len()));
    for e in items {
        let start = format_dt_display(&e.start_at);
        let end = e.end_at.as_deref().map(format_dt_display);
        let when = match end {
            Some(end) => format!("{} - {}", start, end),
            None => start,
        };
        let loc = e.location.as_deref().map(|s| format!("，地点：{}", s)).unwrap_or_default();
        out.push_str(&format!("- {}（id={}，时间：{}{}）\n", e.title, e.id, when, loc));
    }
    out.trim_end().to_string()
}

pub fn delete_event(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let id = match v.get("id").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 id 参数".to_string(),
    };
    let path = events_path(working_dir);
    let mut data: EventsData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let before = data.items.len();
    data.items.retain(|e| e.id != id);
    if data.items.len() == before {
        return format!("未找到日程：id={}", id);
    }
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已删除日程：id={}", id)
}

pub fn update_event(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };
    let id = match v.get("id").and_then(|x| x.as_str()).map(|s| s.trim()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 id 参数".to_string(),
    };
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let start_at = v.get("start_at").and_then(|x| x.as_str()).map(|s| s.trim().to_string());
    let end_at = v.get("end_at").and_then(|x| x.as_str()).map(|s| s.trim().to_string());
    let location = v.get("location").and_then(|x| x.as_str()).map(|s| s.trim().to_string());
    let notes = v.get("notes").and_then(|x| x.as_str()).map(|s| s.trim().to_string());

    if title.is_none()
        && start_at.is_none()
        && end_at.is_none()
        && location.is_none()
        && notes.is_none()
    {
        return "错误：至少提供 title/start_at/end_at/location/notes 中的一个用于更新".to_string();
    }

    let path = events_path(working_dir);
    let mut data: EventsData = match read_json(&path) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut found = None;
    for e in &mut data.items {
        if e.id == id {
            if let Some(t) = title.clone() {
                e.title = t;
            }
            if let Some(s) = start_at.as_deref() {
                let s = s.trim();
                if !s.is_empty() {
                    e.start_at = parse_datetime_to_rfc3339(s).unwrap_or_else(|| s.to_string());
                }
            }
            if let Some(s) = end_at.as_deref() {
                let s = s.trim();
                if s.is_empty() {
                    e.end_at = None;
                } else {
                    e.end_at = Some(parse_datetime_to_rfc3339(s).unwrap_or_else(|| s.to_string()));
                }
            }
            if let Some(s) = location.as_deref() {
                let s = s.trim();
                e.location = if s.is_empty() { None } else { Some(s.to_string()) };
            }
            if let Some(s) = notes.as_deref() {
                let s = s.trim();
                e.notes = if s.is_empty() { None } else { Some(s.to_string()) };
            }
            e.updated_at = Some(now_rfc3339());
            found = Some(e.title.clone());
            break;
        }
    }
    let title = match found {
        Some(t) => t,
        None => return format!("未找到日程：id={}", id),
    };
    if let Err(e) = write_json(&path, &data) {
        return e;
    }
    format!("已更新日程：{}（id={}）", title, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    fn make_temp_dir() -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("crabmate_schedule_test_{}_{}_{}", pid, ts, n));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup_dir(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reminder_add_list_complete_update_delete_flow() {
        let dir = make_temp_dir();
        let out = add_reminder(r#"{"title":"买牛奶"}"#, &dir);
        assert!(out.contains("已添加提醒"));

        let out = list_reminders(r#"{}"#, &dir);
        assert!(out.contains("买牛奶"));

        // extract id from file
        let data: RemindersData = read_json(&reminders_path(&dir)).unwrap();
        assert_eq!(data.items.len(), 1);
        let id = data.items[0].id.clone();

        let out = complete_reminder(&serde_json::json!({ "id": id }).to_string(), &dir);
        assert!(out.contains("已完成提醒"));

        // 默认不包含已完成
        let out = list_reminders(r#"{}"#, &dir);
        assert!(!out.contains("买牛奶"));

        let out = list_reminders(r#"{"include_done":true}"#, &dir);
        assert!(out.contains("买牛奶"));

        // update title + clear due_at
        let data: RemindersData = read_json(&reminders_path(&dir)).unwrap();
        let id = data.items[0].id.clone();
        let out = update_reminder(
            &serde_json::json!({ "id": id, "title": "买燕麦奶", "due_at": "" }).to_string(),
            &dir,
        );
        assert!(out.contains("已更新提醒"));

        let out = delete_reminder(&serde_json::json!({ "id": data.items[0].id }).to_string(), &dir);
        assert!(out.contains("已删除提醒"));

        cleanup_dir(&dir);
    }

    #[test]
    fn reminders_future_days_filter() {
        let dir = make_temp_dir();
        let now = Utc::now();
        let past = (now - chrono::Duration::days(1)).to_rfc3339();
        let soon = (now + chrono::Duration::days(1)).to_rfc3339();
        let later = (now + chrono::Duration::days(10)).to_rfc3339();

        let _ = add_reminder(&serde_json::json!({ "title": "过期", "due_at": past }).to_string(), &dir);
        let _ = add_reminder(&serde_json::json!({ "title": "近期", "due_at": soon }).to_string(), &dir);
        let _ = add_reminder(&serde_json::json!({ "title": "远期", "due_at": later }).to_string(), &dir);

        let out = list_reminders(r#"{"future_days":3}"#, &dir);
        assert!(out.contains("近期"));
        assert!(!out.contains("过期"));
        assert!(!out.contains("远期"));

        cleanup_dir(&dir);
    }

    #[test]
    fn events_add_list_filter_update_delete_flow() {
        let dir = make_temp_dir();
        let now = Utc::now();
        let start = (now + chrono::Duration::days(1)).to_rfc3339();
        let end = (now + chrono::Duration::days(1) + chrono::Duration::hours(2)).to_rfc3339();

        let out = add_event(
            &serde_json::json!({
                "title": "开会",
                "start_at": start,
                "end_at": end,
                "location": "会议室A",
                "notes": "带电脑"
            })
            .to_string(),
            &dir,
        );
        assert!(out.contains("已添加日程"));

        let data: EventsData = read_json(&events_path(&dir)).unwrap();
        assert_eq!(data.items.len(), 1);
        let id = data.items[0].id.clone();

        // future filter should include
        let out = list_events(r#"{"future_days":3}"#, &dir);
        assert!(out.contains("开会"));

        // clear end_at + notes
        let out = update_event(
            &serde_json::json!({ "id": id, "end_at": "", "notes": "" }).to_string(),
            &dir,
        );
        assert!(out.contains("已更新日程"));
        let data: EventsData = read_json(&events_path(&dir)).unwrap();
        assert!(data.items[0].end_at.is_none());
        assert!(data.items[0].notes.is_none());

        let out = delete_event(&serde_json::json!({ "id": data.items[0].id }).to_string(), &dir);
        assert!(out.contains("已删除日程"));

        cleanup_dir(&dir);
    }
}

