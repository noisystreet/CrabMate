//! 左侧会话栏与搜索面板。

use leptos::prelude::*;

use crate::session_ops::SessionContextAnchor;

#[derive(Clone, Copy)]
pub struct SidebarSignals {
    pub sidebar_rail_collapsed: RwSignal<bool>,
    /// 退出 IDE 回到对话时短暂为 `true`，抑制侧栏 0→全宽 的宽度过渡。
    pub sidebar_rail_snap: RwSignal<bool>,
    /// 进入 IDE 前侧栏是否收起；退出 IDE 时恢复。
    pub sidebar_collapsed_before_ide: RwSignal<Option<bool>>,
    pub sidebar_session_query: RwSignal<String>,
    pub global_message_query: RwSignal<String>,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub mobile_nav_open: RwSignal<bool>,
}

impl SidebarSignals {
    pub fn new() -> Self {
        Self {
            sidebar_rail_collapsed: RwSignal::new(false),
            sidebar_rail_snap: RwSignal::new(false),
            sidebar_collapsed_before_ide: RwSignal::new(None),
            sidebar_session_query: RwSignal::new(String::new()),
            global_message_query: RwSignal::new(String::new()),
            sidebar_search_panel_open: RwSignal::new(false),
            sidebar_rail_ctx_menu: RwSignal::new(None),
            session_context_menu: RwSignal::new(None),
            mobile_nav_open: RwSignal::new(false),
        }
    }
}

impl Default for SidebarSignals {
    fn default() -> Self {
        Self::new()
    }
}
