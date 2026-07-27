//! 右侧栏：任务清单与变更预览（对齐 Web 右栏 Workspace/Tasks 文案分区）。

use std::sync::Arc;

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::workspace::tasks_side::TasksData;

use super::sidebar_text::build_tui_workspace_sidebar;

pub(in crate::runtime::tui::run_session) async fn build_tui_workspace_sidebar_extended(
    work_dir: &std::path::Path,
    process_handles: &Arc<ProcessHandles>,
    cfg_holder: &SharedAgentConfig,
    sqlite_conversation_id: Option<&str>,
) -> String {
    let base = build_tui_workspace_sidebar(work_dir);
    let ws_key = work_dir.to_string_lossy().to_string();
    let tasks = process_handles
        .tasks_data_for_workspace_path(ws_key.as_str())
        .await;
    let tasks_block = format_tui_tasks_panel(&tasks);
    let cfg = cfg_holder.read().await;
    let scope = sqlite_conversation_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__default__");
    let changelog_snip = tui_changelog_sidebar_snippet(
        process_handles.workspace_changelog_markdown_for_scope(&cfg, scope),
    );
    format!(
        "{base}\n任务清单\n{tasks_block}\n\n变更预览\n{changelog_snip}\n\n设置：/api-key · /agent · /help\n"
    )
}

fn format_tui_tasks_panel(tasks: &TasksData) -> String {
    let total = tasks.items.len();
    if total == 0 {
        return "0/0 完成\n（空）\n".to_string();
    }
    let done = tasks.items.iter().filter(|it| it.done).count();
    let mut s = format!("{done}/{total} 完成\n");
    for it in tasks.items.iter().take(12) {
        let mark = if it.done { "[x]" } else { "[ ]" };
        let title = truncate_chars_with_ellipsis(it.title.as_str(), 36);
        s.push_str(mark);
        s.push(' ');
        s.push_str(&title);
        s.push('\n');
    }
    if total > 12 {
        s.push_str(&format!("… 共 {total} 项\n"));
    }
    s
}

fn tui_changelog_sidebar_snippet(md_result: Result<String, &'static str>) -> String {
    match md_result {
        Ok(md) => {
            let t = md.trim();
            if t.is_empty() {
                "（暂无）\n".to_string()
            } else {
                // 右栏只留短摘要，对齐 Web「变更预览」入口而非整页 Markdown
                format!("{}\n", truncate_chars_with_ellipsis(t, 240))
            }
        }
        Err(reason) => format!("（{reason}）\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_tui_tasks_panel;
    use crate::workspace::tasks_side::{TaskItem, TasksData};

    #[test]
    fn tasks_panel_shows_done_ratio_like_web() {
        let data = TasksData {
            items: vec![
                TaskItem {
                    id: "1".into(),
                    title: "a".into(),
                    done: true,
                },
                TaskItem {
                    id: "2".into(),
                    title: "b".into(),
                    done: false,
                },
            ],
            ..Default::default()
        };
        let s = format_tui_tasks_panel(&data);
        assert!(s.starts_with("1/2 完成"), "{s}");
        assert!(s.contains("[x] a"), "{s}");
        assert!(s.contains("[ ] b"), "{s}");
    }
}
