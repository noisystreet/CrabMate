//! 按会话作用域（`long_term_memory_scope_id` / Web `conversation_id`）累积**工作区内工具写入**的相对路径与 unified diff 摘要，
//! 在每次调用模型前注入一条 `user` 消息（见 [`crate::types::CRABMATE_WORKSPACE_CHANGELIST_NAME`]），减少大仓库下路径猜测。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use similar::TextDiff;

use crate::types::CRABMATE_WORKSPACE_CHANGELIST_NAME;

/// 全局表：`scope_id` → 本会话变更集（进程内；与 SQLite 会话持久化无关）。
static SCOPES: LazyLock<Mutex<HashMap<String, Arc<WorkspaceChangelist>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 返回给定作用域的变更集句柄（不存在则插入空表）。
pub fn changelist_for_scope(scope_id: &str) -> Arc<WorkspaceChangelist> {
    let key = scope_id.trim();
    let key_owned = if key.is_empty() {
        "__default__".to_string()
    } else {
        key.to_string()
    };
    let mut guard = SCOPES.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(key_owned)
        .or_insert_with(|| Arc::new(WorkspaceChangelist::default()))
        .clone()
}

#[derive(Debug, Clone)]
struct TrackedFile {
    /// 本会话内**首次**记录该路径时的内容；`None` 表示当时文件不存在。
    baseline: Option<String>,
    /// 最近一次成功写操作后的磁盘内容；删除后为 `None`。
    current: Option<String>,
}

/// 单作用域变更集（`Arc` 共享，内层 `Mutex` 保护）。
#[derive(Default)]
pub struct WorkspaceChangelist {
    inner: Mutex<ChangelistInner>,
}

#[derive(Default)]
struct ChangelistInner {
    /// 插入顺序（首次触碰）
    order: Vec<String>,
    files: HashMap<String, TrackedFile>,
    /// 每次成功记录后递增；用于跳过重复的注入内容。
    pub(super) revision: u64,
}

impl WorkspaceChangelist {
    /// 工具成功改写工作区文件后调用：`rel_path` 为相对工作区路径（与工具参数一致），`before` 为写前全文（`None` = 不存在）。
    pub fn record_mutation(&self, rel_path: &str, before: Option<String>, after: Option<String>) {
        let rel = rel_path.trim();
        if rel.is_empty() {
            return;
        }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.revision = g.revision.saturating_add(1);
        if let Some(t) = g.files.get_mut(rel) {
            t.current = after;
            return;
        }
        g.order.push(rel.to_string());
        g.files.insert(
            rel.to_string(),
            TrackedFile {
                baseline: before,
                current: after,
            },
        );
    }

    fn snapshot_revision_and_format(&self, max_total_chars: usize) -> (u64, Option<String>) {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let rev = g.revision;
        let body = format_inner(&g, max_total_chars);
        (rev, body)
    }

    /// 与注入模型的变更集正文一致；供 Web **`GET /workspace/changelog`** 等只读展示。
    pub fn snapshot_markdown(&self, max_total_chars: usize) -> (u64, Option<String>) {
        self.snapshot_revision_and_format(max_total_chars)
    }
}

fn format_inner(inner: &ChangelistInner, max_total_chars: usize) -> Option<String> {
    if inner.order.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    lines.push("[CrabMate 会话工作区变更集] 下列路径为本会话内工具写入所触碰的**工作区相对路径**（供后续轮次优先参考，避免猜路径）。".to_string());
    lines.push(String::new());
    lines.push("### 已触碰路径（按首次写入顺序）".to_string());
    for p in &inner.order {
        lines.push(format!("- {p}"));
    }
    lines.push(String::new());
    lines.push("### 相对「会话内首次触碰」基线的 unified diff 摘要".to_string());
    lines.push("（大文件或路径过多时可能截断；完整内容请以 `read_file` 为准。）".to_string());
    lines.push(String::new());

    let mut budget = max_total_chars.saturating_sub(estimate_chars(&lines));
    let mut diff_blocks: Vec<String> = Vec::new();

    for p in &inner.order {
        let Some(t) = inner.files.get(p) else {
            continue;
        };
        let left = t.baseline.as_deref().unwrap_or("");
        let right = match &t.current {
            Some(s) => s.as_str(),
            None => "",
        };
        let header_note = if t.baseline.is_none() && t.current.is_some() {
            "（新建）"
        } else if t.baseline.is_some() && t.current.is_none() {
            "（已删除）"
        } else {
            ""
        };
        let diff = TextDiff::from_lines(left, right);
        let unified = diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{p}"), &format!("b/{p}"))
            .to_string();
        let block = if header_note.is_empty() {
            format!("#### `{p}`\n```diff\n{unified}\n```\n")
        } else {
            format!("#### `{p}` {header_note}\n```diff\n{unified}\n```\n")
        };
        let cost = block.len().div_ceil(2);
        if cost > budget && !diff_blocks.is_empty() {
            diff_blocks.push(format!(
                "\n… 尚有路径未展示（已达 session_workspace_changelist_max_chars≈{max_total_chars} 上限）…"
            ));
            break;
        }
        diff_blocks.push(block);
        budget = budget.saturating_sub(cost);
    }

    lines.push(diff_blocks.join("\n"));
    Some(lines.join("\n"))
}

fn estimate_chars(parts: &[String]) -> usize {
    parts.iter().map(|s| s.len().div_ceil(2)).sum()
}

/// 供 `prepare_messages_for_model`：移除旧的注入条并视需要插入最新摘要。
pub fn sync_changelist_user_message(
    messages: &mut Vec<crate::types::Message>,
    changelist: Option<&WorkspaceChangelist>,
    enabled: bool,
    max_body_chars: usize,
) {
    strip_changelist_messages(messages);

    if !enabled {
        return;
    }
    let Some(cl) = changelist else {
        return;
    };
    let (_rev, Some(body)) = cl.snapshot_revision_and_format(max_body_chars) else {
        return;
    };
    if messages.is_empty() {
        return;
    }
    messages.push(crate::types::Message {
        role: "user".to_string(),
        content: Some(body.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some(CRABMATE_WORKSPACE_CHANGELIST_NAME.to_string()),
        tool_call_id: None,
    });
}

fn strip_changelist_messages(messages: &mut Vec<crate::types::Message>) {
    messages.retain(|m| !crate::types::is_workspace_changelist_injection(m));
}

/// 持久化/返回客户端前移除变更集注入条（与长期记忆注入同理，由运行时每次刷新）。
pub fn strip_workspace_changelist_injections(messages: &mut Vec<crate::types::Message>) {
    messages.retain(|m| !crate::types::is_workspace_changelist_injection(m));
}

/// 写盘成功后：按相对路径读回当前全文并记入变更集（`before` 为写前快照，`None` 表示当时不存在）。
pub fn record_file_state_after_write(
    cl: Option<&Arc<WorkspaceChangelist>>,
    working_dir: &std::path::Path,
    rel_path: &str,
    before: Option<String>,
) {
    let Some(c) = cl else {
        return;
    };
    let rel = rel_path.trim();
    if rel.is_empty() {
        return;
    }
    let after = match crate::tools::resolve_workspace_path_for_read(working_dir, rel) {
        Ok(p) => std::fs::read_to_string(&p).ok(),
        Err(_) => None,
    };
    c.record_mutation(rel, before, after);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_formats_unified_diff() {
        let cl = WorkspaceChangelist::default();
        cl.record_mutation(
            "src/x.rs",
            Some("fn main() {}\n".to_string()),
            Some("fn main() { let _ = 1; }\n".to_string()),
        );
        let (_r, s) = cl.snapshot_revision_and_format(50_000);
        let s = s.expect("body");
        assert!(s.contains("src/x.rs"));
        assert!(s.contains("```diff"));
        assert!(s.contains("-fn main() {}"));
        assert!(s.contains("+fn main() { let _ = 1; }"));
    }

    #[test]
    fn sync_appends_changelist_at_end() {
        let cl = WorkspaceChangelist::default();
        cl.record_mutation(
            "a.txt",
            Some("old\n".to_string()),
            Some("new\n".to_string()),
        );
        let mut msgs = vec![
            crate::types::Message::system_only("sys"),
            crate::types::Message::user_only("hi"),
        ];
        sync_changelist_user_message(&mut msgs, Some(&cl), true, 50_000);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(
            crate::types::message_content_as_str(&msgs[1].content),
            Some("hi")
        );
        assert_eq!(
            msgs[2].name.as_deref(),
            Some(CRABMATE_WORKSPACE_CHANGELIST_NAME)
        );
        assert!(
            crate::types::message_content_as_str(&msgs[2].content)
                .is_some_and(|c| c.contains("a.txt"))
        );
    }
}
