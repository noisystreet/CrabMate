//! TUI 侧栏 / 状态栏字符串拼装（从 [`super`](crate::runtime::tui::run_session) 拆分以降低 `mod.rs` 物理行数）。
//!
//! 左栏 / 右栏文案对齐 Web/Tauri 分区语义（会话列表、工作区路径）；不复刻 DOM。
//! 最近会话行展示标题与 Tauri `nav-session-title` 同源（首条用户消息推导）。

use crate::config::{AgentConfig, SharedAgentConfig};
use crate::conversation_store::ConversationListEntry;
use crate::text_util::truncate_chars_with_ellipsis;

use super::sqlite_session::TuiSqliteSessionState;

/// 从 SQLite 会话态拉取最近会话（id + 标题；失败或未启用时为空）。
pub(in crate::runtime::tui::run_session) fn tui_recent_conversations(
    sess: Option<&TuiSqliteSessionState>,
) -> Vec<ConversationListEntry> {
    sess.and_then(|s| s.list_recent_entries(12).ok())
        .unwrap_or_default()
}

/// 左侧会话栏（对齐 Web `nav-rail`：会话在左、最近列表、标题文案）。
pub(in crate::runtime::tui::run_session) fn build_tui_session_sidebar(
    tui_load_on_start: bool,
    session_file_exists: bool,
    message_count: usize,
    sqlite_conversation_id: Option<&str>,
    recent: &[ConversationListEntry],
) -> String {
    let mut out = String::from("会话\n\n");
    out.push_str("最近会话\n");
    let current = sqlite_conversation_id
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if recent.is_empty() {
        if let Some(id) = current {
            out.push_str("* ");
            out.push_str(&truncate_chars_with_ellipsis(id, 36));
            out.push('\n');
        } else {
            let sess = if session_file_exists { "有" } else { "无" };
            let load = if tui_load_on_start { "开" } else { "关" };
            out.push_str(&format!(
                "（未启用 SQLite）\n本地文件 {sess} · 启动加载 {load}\n"
            ));
        }
    } else {
        for entry in recent.iter().take(12) {
            let id = entry.id.trim();
            if id.is_empty() {
                continue;
            }
            let title = entry.title.trim();
            let label = if title.is_empty() {
                truncate_chars_with_ellipsis(id, 36)
            } else {
                truncate_chars_with_ellipsis(title, 36)
            };
            if current == Some(id) {
                out.push_str("* ");
            } else {
                out.push_str("  ");
            }
            out.push_str(&label);
            out.push('\n');
        }
        if recent.len() > 12 {
            out.push_str(&format!("  … 共 {} 个\n", recent.len()));
        }
    }
    out.push('\n');
    out.push_str(&format!("{message_count} 条\n"));
    out
}

/// 右侧工作区栏：仅路径短示（无任务清单 / 变更预览 / 快捷键帮助）。
pub(in crate::runtime::tui::run_session) fn build_tui_workspace_sidebar(
    work_dir: &std::path::Path,
) -> String {
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 40);
    format!("工作区\n{wd_short}\n")
}

/// 与 Web 底栏「角色」下拉一致：显式 `/agent set` 显示 id；否则 default / default (配置 id）。
pub(in crate::runtime::tui::run_session) fn tui_status_role_label(
    agent_role_owned: &Option<String>,
    cfg: &AgentConfig,
) -> String {
    if let Some(id) = agent_role_owned
        .as_ref()
        .map(|x| x.trim())
        .filter(|s| !s.is_empty())
    {
        return id.to_string();
    }
    match cfg
        .roles_prompts
        .default_agent_role_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => format!("default ({id})"),
        None => "default".to_string(),
    }
}

/// Web 底栏 chips 段（不含末尾「就绪 / 模型生成中…」等运行态）。
pub(in crate::runtime::tui::run_session) async fn tui_status_chips_line(
    cfg_holder: &SharedAgentConfig,
    agent_role_owned: &Option<String>,
) -> String {
    let g = cfg_holder.read().await;
    let model_id = g.llm.model.as_str();
    let role = tui_status_role_label(agent_role_owned, &g);
    format!("模型 · {model_id} · 角色 · {role}")
}

/// 带会话消息粗估的底栏 chips（与 Web 上下文芯片对齐）。
pub(in crate::runtime::tui::run_session) async fn tui_status_chips_line_with_messages(
    cfg_holder: &SharedAgentConfig,
    agent_role_owned: &Option<String>,
    messages: &[crate::types::Message],
) -> String {
    let base = tui_status_chips_line(cfg_holder, agent_role_owned).await;
    let g = cfg_holder.read().await;
    let ctx = crate::runtime::context_usage::context_usage_chip_line(&g, messages);
    format!("{base} · {ctx}")
}

/// Web / Tauri `StatusBarRunIndicator` 文案（底栏最右；chips 在左侧单独渲染）。
pub(in crate::runtime::tui::run_session) fn tui_status_run_ready() -> &'static str {
    "就绪"
}

pub(in crate::runtime::tui::run_session) fn tui_status_run_model_busy() -> &'static str {
    "模型生成中…"
}

pub(in crate::runtime::tui::run_session) fn tui_status_run_tool_busy() -> &'static str {
    "工具执行中…"
}

pub(in crate::runtime::tui::run_session) fn tui_status_run_error(detail: &str) -> String {
    format!("错误: {detail}")
}

pub(in crate::runtime::tui::run_session) fn tui_use_ansi_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationListEntry;

    #[test]
    fn session_sidebar_lists_titles_with_current_star() {
        let s = build_tui_session_sidebar(
            true,
            true,
            3,
            Some("conv-current"),
            &[
                ConversationListEntry {
                    id: "conv-current".into(),
                    title: "分析 README".into(),
                },
                ConversationListEntry {
                    id: "conv-older".into(),
                    title: "修编译错误".into(),
                },
                ConversationListEntry {
                    id: "conv-third".into(),
                    title: "新会话".into(),
                },
            ],
        );
        assert!(s.contains("最近会话"), "{s}");
        assert!(s.contains("* 分析 README"), "{s}");
        assert!(s.contains("  修编译错误"), "{s}");
        assert!(s.contains("3 条"), "{s}");
        assert!(!s.contains("conv-current"), "show title not raw id: {s}");
        assert!(!s.contains("/conv"), "{s}");
    }

    #[test]
    fn workspace_sidebar_is_path_only_no_hotkeys_wall() {
        let s = build_tui_workspace_sidebar(std::path::Path::new("/tmp/ws"));
        assert!(s.contains("工作区"), "{s}");
        assert!(s.contains("/tmp/ws"), "{s}");
        assert!(!s.contains("Enter"), "{s}");
        assert!(!s.contains("快捷键"), "{s}");
        assert!(!s.contains("/help"), "{s}");
        assert!(!s.contains("任务清单"), "{s}");
        assert!(!s.contains("变更预览"), "{s}");
        assert!(!s.contains("已加载工具"), "{s}");
    }
}
