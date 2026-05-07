//! 右侧栏：侧栏任务与变更集附录（与 Web **`/tasks`**、**`/workspace/changelog`** 共用 [`ProcessHandles`]）。

use std::sync::Arc;

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::workspace::tasks_side::TasksData;

use super::build_tui_workspace_sidebar;

pub(in crate::runtime::tui::run_session) async fn build_tui_workspace_sidebar_extended(
    work_dir: &std::path::Path,
    tool_count: usize,
    cli_no_stream: bool,
    process_handles: &Arc<ProcessHandles>,
    cfg_holder: &SharedAgentConfig,
    sqlite_conversation_id: Option<&str>,
) -> String {
    let base = build_tui_workspace_sidebar(work_dir, tool_count, cli_no_stream);
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
        "{base}\n\n── Web 同源 ──\n侧栏任务（内存 · `/tasks`）\n{tasks_block}\n\n工作区变更集（`/workspace/changelog`）\n{changelog_snip}\n\n设置：`/api-key` · `/agent` · `/help`（同 REPL）；模型见底栏。"
    )
}

fn format_tui_tasks_panel(tasks: &TasksData) -> String {
    if tasks.items.is_empty() {
        return "（空；Web 侧栏写入后此处同步）".to_string();
    }
    let mut s = String::new();
    for it in tasks.items.iter().take(12) {
        let mark = if it.done { "[x]" } else { "[ ]" };
        let title = truncate_chars_with_ellipsis(it.title.as_str(), 36);
        s.push_str(mark);
        s.push(' ');
        s.push_str(&title);
        s.push('\n');
    }
    if tasks.items.len() > 12 {
        s.push_str(&format!("… 共 {} 项\n", tasks.items.len()));
    }
    s
}

fn tui_changelog_sidebar_snippet(md_result: Result<String, &'static str>) -> String {
    match md_result {
        Ok(md) => {
            let t = md.trim();
            if t.is_empty() {
                "（暂无变更记录）".to_string()
            } else {
                truncate_chars_with_ellipsis(t, 560)
            }
        }
        Err(reason) => format!("（{reason}）"),
    }
}
