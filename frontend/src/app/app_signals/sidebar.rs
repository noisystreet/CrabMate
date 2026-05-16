//! 左侧会话栏与搜索面板。

use leptos::prelude::*;

use crate::app_prefs::{SIDEBAR_RAIL_COLLAPSED_KEY, load_bool_key};
use crate::session_ops::SessionContextAnchor;

#[derive(Clone, Copy)]
pub struct SidebarSignals {
    pub sidebar_rail_collapsed: RwSignal<bool>,
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
            sidebar_rail_collapsed: RwSignal::new(load_bool_key(SIDEBAR_RAIL_COLLAPSED_KEY, false)),
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
