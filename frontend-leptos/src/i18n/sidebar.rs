use super::Locale;

// --- 侧栏 / 移动顶栏 ---

pub fn brand_sub(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本地 Agent",
        Locale::En => "Local agent",
    }
}

pub fn nav_new_chat(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新对话",
        Locale::En => "New chat",
    }
}

pub fn nav_sidebar_collapse_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起会话栏",
        Locale::En => "Collapse session sidebar",
    }
}

pub fn nav_sidebar_expand_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开会话栏",
        Locale::En => "Expand session sidebar",
    }
}

pub fn nav_manage_sessions(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "管理会话…",
        Locale::En => "Manage sessions…",
    }
}

pub fn nav_filter_sessions(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "筛选会话",
        Locale::En => "Filter sessions",
    }
}

pub fn nav_ph_filter(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "按标题筛选…",
        Locale::En => "Filter by title…",
    }
}

pub fn nav_search_messages(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "搜索消息",
        Locale::En => "Search messages",
    }
}

pub fn nav_ph_global_search(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "全文搜索（本地）…",
        Locale::En => "Full-text search (local)…",
    }
}

pub fn nav_recent(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话（置顶 · 收藏 · 活动时间）",
        Locale::En => "Sessions (pinned · starred · activity)",
    }
}

pub fn nav_no_message_hits(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无匹配消息",
        Locale::En => "No matching messages",
    }
}

pub fn nav_search_hits_region(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "消息搜索结果",
        Locale::En => "Message search results",
    }
}

/// 侧栏会话列表空白处右键菜单：打开筛选 + 跨会话搜索。
pub fn nav_rail_ctx_filter_and_search(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "筛选会话与搜索消息…",
        Locale::En => "Filter sessions & search messages…",
    }
}

/// 侧栏会话列表空白处右键菜单：打开主区当前会话查找条。
pub fn nav_rail_ctx_find_in_chat(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前会话内查找…",
        Locale::En => "Find in current chat…",
    }
}

pub fn nav_hide_search_panel(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起筛选与搜索",
        Locale::En => "Hide filter & search",
    }
}

pub fn nav_hide_search_panel_aria(l: Locale) -> &'static str {
    nav_hide_search_panel(l)
}

/// 悬停于会话列表区时的提示：如何打开筛选/搜索。
pub fn nav_rail_scroll_search_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在列表空白处右键可打开筛选与搜索",
        Locale::En => "Right-click empty area here for filter & search",
    }
}

pub fn ctx_export_json(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 JSON",
        Locale::En => "Export JSON",
    }
}

pub fn ctx_export_md(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 Markdown",
        Locale::En => "Export Markdown",
    }
}

pub fn ctx_delete_session(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除会话",
        Locale::En => "Delete session",
    }
}

pub fn ctx_star_session(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收藏",
        Locale::En => "Star",
    }
}

pub fn ctx_unstar_session(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "取消收藏",
        Locale::En => "Unstar",
    }
}

pub fn ctx_pin_session(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "置顶",
        Locale::En => "Pin to top",
    }
}

pub fn ctx_unpin_session(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "取消置顶",
        Locale::En => "Unpin",
    }
}

pub fn session_badge_star_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已收藏",
        Locale::En => "Starred",
    }
}

pub fn session_badge_pin_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已置顶",
        Locale::En => "Pinned",
    }
}

pub fn mobile_open_menu(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开菜单",
        Locale::En => "Open menu",
    }
}
