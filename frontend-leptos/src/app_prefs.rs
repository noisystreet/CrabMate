//! 本机偏好与布局常量：localStorage 键、侧栏视图、状态栏展示用合并逻辑。

use crate::api::StatusData;

pub const WORKSPACE_WIDTH_KEY: &str = "agent-demo-workspace-width";
pub const WORKSPACE_VISIBLE_KEY: &str = "agent-demo-workspace-visible";
pub const TASKS_VISIBLE_KEY: &str = "agent-demo-tasks-visible";
/// 右列侧栏视图：`none` | `workspace` | `tasks` | `debug`（与旧版双开关互斥，仅其一展示）。
pub const SIDE_PANEL_VIEW_KEY: &str = "agent-demo-side-panel-view";
pub const STATUS_BAR_VISIBLE_KEY: &str = "agent-demo-status-bar-visible";
pub const THEME_KEY: &str = "crabmate-theme";
/// 界面语言：`zh-Hans` | `en`（与 `<html lang>` 一致）。
pub const LOCALE_KEY: &str = "crabmate-locale";
/// 为 `true` 时显示页面径向渐变光晕；`false` 时仅纯色背景（`data-bg-decor="plain"`）。
pub const BG_DECOR_KEY: &str = "crabmate-bg-decor";
pub const AGENT_ROLE_KEY: &str = "agent-demo-agent-role";
/// 聊天列「规划 / 工具时间线」面板是否展开（`localStorage` bool）。
pub const TIMELINE_PANEL_EXPANDED_KEY: &str = "crabmate-timeline-panel-expanded";
/// 桌面端左侧会话栏是否收起（`true` 为收起；窄屏抽屉菜单不受此键影响）。
pub const SIDEBAR_RAIL_COLLAPSED_KEY: &str = "crabmate-sidebar-rail-collapsed";
pub const DEFAULT_SIDE_WIDTH: f64 = 280.0;
pub const MIN_SIDE_WIDTH: f64 = 200.0;
pub const MAX_SIDE_WIDTH: f64 = 560.0;
/// 为左侧对话列预留的最小宽度（视口过窄时仍允许侧栏拖到 `MIN_SIDE_WIDTH`，由 flex 挤压主列）。
pub const MIN_CHAT_RESERVE_PX: f64 = 240.0;
pub const AUTO_SCROLL_RESUME_GAP_PX: i32 = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidePanelView {
    None,
    Workspace,
    Tasks,
    /// 思维与工具调试台（与工作区共用右列宽度与 `side-pane` 布局）。
    DebugConsole,
}

pub fn load_side_panel_view() -> SidePanelView {
    let Some(st) = local_storage() else {
        return SidePanelView::Workspace;
    };
    if let Ok(Some(v)) = st.get_item(SIDE_PANEL_VIEW_KEY) {
        return match v.trim() {
            "none" => SidePanelView::None,
            "tasks" => SidePanelView::Tasks,
            "workspace" => SidePanelView::Workspace,
            "debug" => SidePanelView::DebugConsole,
            _ => SidePanelView::Workspace,
        };
    }
    let wv = load_bool_key(WORKSPACE_VISIBLE_KEY, true);
    let tv = load_bool_key(TASKS_VISIBLE_KEY, false);
    let migrated = if wv {
        SidePanelView::Workspace
    } else if tv {
        SidePanelView::Tasks
    } else {
        SidePanelView::None
    };
    let slug = match migrated {
        SidePanelView::None => "none",
        SidePanelView::Workspace => "workspace",
        SidePanelView::Tasks => "tasks",
        SidePanelView::DebugConsole => "debug",
    };
    let _ = st.set_item(SIDE_PANEL_VIEW_KEY, slug);
    migrated
}

pub fn store_side_panel_view(v: SidePanelView) {
    if let Some(st) = local_storage() {
        let slug = match v {
            SidePanelView::None => "none",
            SidePanelView::Workspace => "workspace",
            SidePanelView::Tasks => "tasks",
            SidePanelView::DebugConsole => "debug",
        };
        let _ = st.set_item(SIDE_PANEL_VIEW_KEY, slug);
    }
}

pub fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// 状态栏「模型」：本机保存的 `client_llm.model` 非空时优先，否则用 `/status`。
pub fn status_bar_effective_model(server: Option<&StatusData>, stored_model: &str) -> String {
    let t = stored_model.trim();
    if !t.is_empty() {
        t.to_string()
    } else {
        server
            .map(|d| d.model.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

/// 状态栏「base_url」：本机 `client_llm.api_base` 非空时优先，否则用 `/status`。
pub fn status_bar_effective_api_base(server: Option<&StatusData>, stored_api_base: &str) -> String {
    let t = stored_api_base.trim();
    if !t.is_empty() {
        t.to_string()
    } else {
        server
            .map(|d| d.api_base.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

pub fn clamp_side_width_for_viewport(w: f64) -> f64 {
    let win = web_sys::window()
        .and_then(|win| win.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let max_w = (win - MIN_CHAT_RESERVE_PX).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
    w.clamp(MIN_SIDE_WIDTH, max_w)
}

pub fn load_f64_key(key: &str, default: f64) -> f64 {
    let Some(st) = local_storage() else {
        return clamp_side_width_for_viewport(default);
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return clamp_side_width_for_viewport(default);
    };
    match v.parse::<f64>() {
        Ok(n) => clamp_side_width_for_viewport(n),
        _ => clamp_side_width_for_viewport(default),
    }
}

pub fn load_bool_key(key: &str, default: bool) -> bool {
    let Some(st) = local_storage() else {
        return default;
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return default;
    };
    !(v == "0" || v == "false")
}

pub fn store_bool_key(key: &str, v: bool) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(key, if v { "1" } else { "0" });
    }
}

pub fn store_f64_key(key: &str, v: f64) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(key, &v.to_string());
    }
}
