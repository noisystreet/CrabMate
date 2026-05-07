//! TUI 侧栏 / 状态栏字符串拼装（从 [`super`](crate::runtime::tui::run_session) 拆分以降低 `mod.rs` 物理行数）。

use crate::config::{AgentConfig, SharedAgentConfig};
use crate::text_util::truncate_chars_with_ellipsis;

/// 左侧会话栏（对齐 Web：会话在左）。
pub(in crate::runtime::tui::run_session) fn build_tui_session_sidebar(
    tui_load_on_start: bool,
    session_file_exists: bool,
    message_count: usize,
    sqlite_conversation_id: Option<&str>,
) -> String {
    let sess = if session_file_exists { "有" } else { "无" };
    let load = if tui_load_on_start { "开" } else { "关" };
    let sqlite_block = if let Some(id) = sqlite_conversation_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let short = truncate_chars_with_ellipsis(id, 40);
        format!("\n\nSQLite 会话\n{short}\n（/conv、/branch）")
    } else {
        String::new()
    };
    format!(
        "会话\n\n会话文件\ntui_session.json：{sess}\n启动加载：{load}\n\n内存消息\n{message_count} 条（含 system / 工具）{sqlite_block}\n\n中区仅展示 transcript\n可见尾部",
    )
}

/// 右侧工作区栏 + 任务提示（对齐 Web：工作区在右）。
pub(in crate::runtime::tui::run_session) fn build_tui_workspace_sidebar(
    work_dir: &std::path::Path,
    tool_count: usize,
    cli_no_stream: bool,
) -> String {
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 40);
    format!(
        "工作区\n{wd_short}\n\n聚焦本栏按 Enter：浏览/编辑路径\n（与 Web 侧栏工作区、REPL /workspace 同源校验）\n\n快捷键\n{}\n\n敏感工具审批：全屏 Modal（↑↓ · Enter · Esc · 1/2/3）。\n\n已加载工具：{tool_count} 个",
        tui_keyboard_help_compact(cli_no_stream),
    )
}

/// 原底栏文案迁至侧栏；与 `--no-stream` 对齐 REPL 提示。
pub(in crate::runtime::tui::run_session) fn tui_keyboard_help_compact(
    cli_no_stream: bool,
) -> String {
    let mut s = String::from(
        "Enter 发送 · 空行 q · Ctrl+C · /help · Tab 切焦点 · 鼠标点面板 · 聊天区 PgUp/PgDn · 右侧滚动条拖动",
    );
    if cli_no_stream {
        s.push_str(" · --no-stream");
    } else {
        s.push_str(" · 流式（不写 stdout）");
    }
    s
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
    let base = truncate_chars_with_ellipsis(g.llm.api_base.trim(), 44);
    let role = tui_status_role_label(agent_role_owned, &g);
    format!("模型 · {model_id} · base_url · {base} · 角色 · {role}")
}

pub(in crate::runtime::tui::run_session) fn tui_status_bar_with_run(
    chips: &str,
    run: &str,
) -> String {
    format!("{chips} · {run}")
}

/// Web `status_model_running` 文案 + TUI 补充的消息条数。
pub(in crate::runtime::tui::run_session) fn tui_status_suffix_model_busy_lines(
    msg_len: usize,
) -> String {
    format!("模型生成中… · {msg_len} 条")
}

pub(in crate::runtime::tui::run_session) fn tui_use_ansi_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}
