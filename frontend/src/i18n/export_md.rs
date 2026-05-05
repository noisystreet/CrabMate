use super::Locale;

// --- Markdown 导出（前端下载用，与 CLI 中文标题可并存）---

pub fn export_md_title_full(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "# CrabMate 聊天记录\n\n",
        Locale::En => "# CrabMate chat export\n\n",
    }
}

#[allow(dead_code)] // 供 `session_export` 按 id 子集导出测试保留；Web UI 已移除多选导出。
pub fn export_md_title_selection(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "# CrabMate 聊天记录（已选消息）\n\n",
        Locale::En => "# CrabMate chat export (selected messages)\n\n",
    }
}

pub fn export_md_heading_user(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "## 用户",
        Locale::En => "## User",
    }
}

pub fn export_md_heading_assistant(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "## 助手",
        Locale::En => "## Assistant",
    }
}

pub fn export_md_heading_tool(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "## 工具",
        Locale::En => "## Tool",
    }
}

pub fn export_md_heading_other(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "## 其它",
        Locale::En => "## Other",
    }
}

pub fn export_md_heading_timeline(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "## 时间线",
        Locale::En => "## Timeline",
    }
}
