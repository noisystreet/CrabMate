//! 界面文案与语言。当前为 **zh-Hans** / **en** 静态表；新文案请在此集中维护，便于后续接 ICU / 远程词条。

use crate::app_prefs::{LOCALE_KEY, local_storage};

/// 界面语言（与 `<html lang>` 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    ZhHans,
    En,
}

impl Locale {
    pub fn from_storage_slug(s: &str) -> Self {
        match s.trim() {
            "en" => Locale::En,
            _ => Locale::ZhHans,
        }
    }

    pub fn html_lang(self) -> &'static str {
        match self {
            Locale::ZhHans => "zh-Hans",
            Locale::En => "en",
        }
    }

    pub fn storage_slug(self) -> &'static str {
        match self {
            Locale::ZhHans => "zh-Hans",
            Locale::En => "en",
        }
    }
}

pub fn load_locale_from_storage() -> Locale {
    local_storage()
        .and_then(|s| s.get_item(LOCALE_KEY).ok().flatten())
        .map(|v| Locale::from_storage_slug(&v))
        .unwrap_or(Locale::ZhHans)
}

pub fn store_locale_slug(slug: &str) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(LOCALE_KEY, slug);
    }
}

// --- 设置弹窗 ---

pub fn settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置",
        Locale::En => "Settings",
    }
}

pub fn settings_badge_local(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本机",
        Locale::En => "Local",
    }
}

pub fn settings_close(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭",
        Locale::En => "Close",
    }
}

pub fn settings_intro(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "主题与页面背景保存在本机（localStorage）。模型网关与 API 密钥也可仅存本机；发消息时会在 JSON 中附带覆盖项，请仅在可信环境（HTTPS）使用。"
        }
        Locale::En => {
            "Theme and page background are stored locally (localStorage). Model endpoint and API key can also stay in the browser; they are sent as JSON overrides with each message—use only on trusted connections (HTTPS)."
        }
    }
}

pub fn settings_block_language(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "界面语言",
        Locale::En => "Interface language",
    }
}

pub fn settings_lang_zh(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "简体中文",
        Locale::En => "Chinese (Simplified)",
    }
}

pub fn settings_lang_en(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "English",
        Locale::En => "English",
    }
}

pub fn settings_block_theme(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "主题",
        Locale::En => "Theme",
    }
}

pub fn settings_theme_dark(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "深色",
        Locale::En => "Dark",
    }
}

pub fn settings_theme_light(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "浅色",
        Locale::En => "Light",
    }
}

pub fn settings_block_bg(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "页面背景",
        Locale::En => "Page background",
    }
}

pub fn settings_bg_glow(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示背景光晕（径向渐变）",
        Locale::En => "Show background glow (radial gradients)",
    }
}

pub fn settings_block_llm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型网关（可选覆盖）",
        Locale::En => "Model endpoint (optional override)",
    }
}

pub fn settings_llm_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "留空则使用服务端配置与环境变量 API_KEY。API 密钥使用密码框，不会以明文显示。"
        }
        Locale::En => {
            "Leave empty to use server config and the API_KEY environment variable. The API key field is masked."
        }
    }
}

pub fn settings_label_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API 基址（api_base）",
        Locale::En => "API base (api_base)",
    }
}

pub fn settings_ph_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "例如 https://api.deepseek.com/v1",
        Locale::En => "e.g. https://api.deepseek.com/v1",
    }
}

pub fn settings_label_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型名称（model）",
        Locale::En => "Model name (model)",
    }
}

pub fn settings_ph_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "例如 deepseek-chat",
        Locale::En => "e.g. deepseek-chat",
    }
}

pub fn settings_label_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API 密钥（覆盖 API_KEY）",
        Locale::En => "API key (overrides API_KEY)",
    }
}

pub fn settings_ph_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "留空保留已存密钥；填写新密钥后点保存",
        Locale::En => "Leave blank to keep saved key; enter new key and Save",
    }
}

pub fn settings_key_saved_note(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前已在本机保存密钥（不会回显到输入框）。",
        Locale::En => "A key is saved locally (not shown in the field).",
    }
}

pub fn settings_save_llm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存模型设置",
        Locale::En => "Save model settings",
    }
}

pub fn settings_clear_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "清除已存密钥",
        Locale::En => "Clear saved key",
    }
}

pub fn settings_saved_browser(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已保存到本机浏览器",
        Locale::En => "Saved in this browser",
    }
}

pub fn settings_cleared_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已清除本机保存的密钥",
        Locale::En => "Cleared locally saved key",
    }
}

pub fn settings_block_shortcuts(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "键盘",
        Locale::En => "Keyboard",
    }
}

pub fn settings_shortcuts_body(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "Esc：关闭最上层弹层（菜单、查找栏、设置等）。在输入框外按 End：滚动到对话底部。对话框打开时 Tab 在框内循环。"
        }
        Locale::En => {
            "Esc: close the top overlay (menus, find bar, settings, etc.). End (outside inputs): scroll chat to bottom. Tab cycles within an open dialog."
        }
    }
}

// --- 会话列表模态 ---

pub fn session_modal_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话",
        Locale::En => "Sessions",
    }
}

pub fn session_modal_badge(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本地",
        Locale::En => "Local",
    }
}

pub fn session_modal_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "本地保存在浏览器；可导出为与 CLI save-session 同形的 JSON / Markdown 下载。"
        }
        Locale::En => "Stored in the browser; export as JSON / Markdown matching CLI save-session.",
    }
}

pub fn session_row_msg_count(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 条"),
        Locale::En => {
            if n == 1 {
                "1 message".to_string()
            } else {
                format!("{n} messages")
            }
        }
    }
}

pub fn session_row_rename_title_attr(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重命名",
        Locale::En => "Rename",
    }
}

pub fn session_row_rename_button(l: Locale) -> &'static str {
    session_row_rename_title_attr(l)
}

pub fn session_prompt_title_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话标题",
        Locale::En => "Session title",
    }
}

pub fn session_row_export_json_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 JSON（ChatSessionFile v1）",
        Locale::En => "Export JSON (ChatSessionFile v1)",
    }
}

pub fn session_row_export_md_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 Markdown",
        Locale::En => "Export Markdown",
    }
}

pub fn session_row_delete_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除此会话",
        Locale::En => "Delete this session",
    }
}

pub fn session_row_delete_button(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除",
        Locale::En => "Delete",
    }
}

// --- 变更集模态 ---

pub fn changelist_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话工作区变更",
        Locale::En => "Workspace changes (session)",
    }
}

pub fn changelist_refresh(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "刷新",
        Locale::En => "Refresh",
    }
}

pub fn changelist_loading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载中…",
        Locale::En => "Loading…",
    }
}

pub fn changelist_rev(l: Locale, n: u64) -> String {
    match l {
        Locale::ZhHans => format!("rev {n}"),
        Locale::En => format!("rev {n}"),
    }
}

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
        Locale::ZhHans => "最近",
        Locale::En => "Recent",
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

pub fn mobile_open_menu(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开菜单",
        Locale::En => "Open menu",
    }
}

// --- 查找栏 ---

pub fn chat_find_region(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在当前会话中查找",
        Locale::En => "Find in this conversation",
    }
}

pub fn chat_find_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查找",
        Locale::En => "Find",
    }
}

pub fn chat_find_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前会话消息…",
        Locale::En => "Messages in this chat…",
    }
}

pub fn chat_find_no_match(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无匹配",
        Locale::En => "No match",
    }
}

pub fn chat_find_prev_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上一条匹配",
        Locale::En => "Previous match",
    }
}

pub fn chat_find_next_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "下一条匹配",
        Locale::En => "Next match",
    }
}

pub fn chat_find_close_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起查找栏",
        Locale::En => "Close find bar",
    }
}

pub fn chat_find_close_aria(l: Locale) -> &'static str {
    chat_find_close_title(l)
}

// --- 侧栏工具栏 / 工作区 ---

pub fn side_resize_handle(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "拖拽调整右列宽度",
        Locale::En => "Drag to resize right column",
    }
}

pub fn side_toolbar_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "视图与设置",
        Locale::En => "View and settings",
    }
}

pub fn side_view_menu_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择侧栏：隐藏 / 工作区 / 任务",
        Locale::En => "Side panel: hide / workspace / tasks",
    }
}

pub fn side_view_label(l: Locale, open: bool) -> String {
    match l {
        Locale::ZhHans => {
            let s = if open { "▴" } else { "▾" };
            format!("视图{s}")
        }
        Locale::En => {
            let s = if open { "▴" } else { "▾" };
            format!("View{s}")
        }
    }
}

pub fn side_view_menu_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "侧栏视图",
        Locale::En => "Side panel view",
    }
}

pub fn side_panel_hide(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "隐藏侧栏",
        Locale::En => "Hide panel",
    }
}

pub fn side_panel_workspace(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区",
        Locale::En => "Workspace",
    }
}

pub fn side_panel_tasks(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "任务",
        Locale::En => "Tasks",
    }
}

pub fn side_status_btn_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态栏",
        Locale::En => "Status bar",
    }
}

pub fn side_status_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态",
        Locale::En => "Status",
    }
}

pub fn side_settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "外观与背景",
        Locale::En => "Appearance",
    }
}

pub fn side_settings_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置",
        Locale::En => "Settings",
    }
}

pub fn tasks_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "任务清单",
        Locale::En => "Tasks",
    }
}

pub fn tasks_loading(l: Locale) -> &'static str {
    changelist_loading(l)
}

pub fn tasks_error(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "错误",
        Locale::En => "Error",
    }
}

pub fn tasks_done_ratio(l: Locale, done: usize, total: usize) -> String {
    match l {
        Locale::ZhHans => format!("{done}/{total} 完成"),
        Locale::En => format!("{done}/{total} done"),
    }
}

pub fn tasks_refresh(l: Locale) -> &'static str {
    changelist_refresh(l)
}

pub fn tasks_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载任务",
        Locale::En => "Loading tasks",
    }
}

pub fn ws_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载工作区",
        Locale::En => "Loading workspace",
    }
}

pub fn ws_root_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区根目录",
        Locale::En => "Workspace root",
    }
}

pub fn ws_input_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "绝对路径（允许根内）；浏览选目录将自动生效，手动输入后按 Enter",
        Locale::En => {
            "Absolute path (within allowed roots); pick applies automatically, or type and press Enter"
        }
    }
}

pub fn ws_input_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在运行 serve 的机器上选目录后会立即提交；亦可手输路径后按 Enter",
        Locale::En => {
            "Picking a folder on the serve host submits immediately; or type a path and press Enter"
        }
    }
}

pub fn ws_path_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写目录路径。",
        Locale::En => "Please enter a directory path.",
    }
}

pub fn ws_browse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在运行 serve 的机器上打开系统选目录对话框",
        Locale::En => "Open folder picker on the serve host",
    }
}

pub fn ws_pick_none(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "未选择目录，或服务端无法弹窗（无图形/无头/SSH 远端）。请手动填写路径后按 Enter。"
        }
        Locale::En => {
            "No folder chosen, or the server cannot show a dialog (headless/SSH). Enter a path manually and press Enter."
        }
    }
}

pub fn ws_browse_busy(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "…",
        Locale::En => "…",
    }
}

pub fn ws_browse_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "浏览…",
        Locale::En => "Browse…",
    }
}

pub fn ws_refresh_list(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "刷新列表",
        Locale::En => "Refresh list",
    }
}

pub fn ws_changelog_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查看本会话工具写入的 unified diff 摘要（与注入模型的变更集同源）",
        Locale::En => "View unified diff summary for this session (same as model changelist)",
    }
}

pub fn ws_changelog_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "变更预览",
        Locale::En => "Change preview",
    }
}

// --- 聊天列空态 / 输入区 ---

pub fn chat_empty_lead(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在下方输入消息，Enter 发送，Shift+Enter 换行。",
        Locale::En => "Type below: Enter to send, Shift+Enter for newline.",
    }
}

pub fn chat_empty_tip1(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "左侧可新建对话、切换最近会话，或「管理会话」导出与重命名。",
        Locale::En => {
            "Use the left rail for new chat, recent sessions, or Manage sessions to export/rename."
        }
    }
}

pub fn chat_empty_tip2(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "侧栏展开时工具栏在右列顶部；「隐藏侧栏」后右侧贴边纵向三键，同宽铺满一条，无额外围框。视图菜单可在隐藏、工作区、任务之间切换。"
        }
        Locale::En => {
            "With the side panel open, tools are on the top of the right column; when hidden, three buttons stack on the right edge. The view menu switches hide / workspace / tasks."
        }
    }
}

pub fn composer_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "输入消息，Enter 发送 / Shift+Enter 换行…",
        Locale::En => "Message: Enter to send / Shift+Enter newline…",
    }
}

pub fn composer_stop(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "停止",
        Locale::En => "Stop",
    }
}

pub fn composer_send_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "发送",
        Locale::En => "Send",
    }
}

pub fn bubble_md_toggle_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "多选消息导出 Markdown（聊天区亦可右键）",
        Locale::En => "Select messages to export Markdown (or right-click in chat)",
    }
}

pub fn bubble_md_toggle_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "多选导出 Markdown",
        Locale::En => "Select for Markdown export",
    }
}

pub fn chat_find_toggle_title(l: Locale) -> &'static str {
    chat_find_region(l)
}

pub fn chat_find_toggle_aria(l: Locale) -> &'static str {
    chat_find_region(l)
}

// --- 聊天区右键菜单 ---

pub fn chat_ctx_menu_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "聊天区菜单",
        Locale::En => "Chat menu",
    }
}

pub fn chat_ctx_copy_selection(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "复制选中文字",
        Locale::En => "Copy selection",
    }
}

pub fn chat_ctx_md_multi(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "多选导出 Markdown…",
        Locale::En => "Multi-select Markdown export…",
    }
}

pub fn chat_ctx_select_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "全选消息",
        Locale::En => "Select all messages",
    }
}

pub fn chat_ctx_clear_sel(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "清除选择",
        Locale::En => "Clear selection",
    }
}

pub fn chat_ctx_export_md_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出已选为 Markdown",
        Locale::En => "Export selection as Markdown",
    }
}

pub fn chat_ctx_export_md_n(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("导出已选为 Markdown（{n} 条）"),
        Locale::En => format!("Export {n} messages as Markdown"),
    }
}

pub fn chat_ctx_exit_multi(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "退出多选",
        Locale::En => "Exit multi-select",
    }
}

// --- 消息气泡 ---

pub fn msg_role_user(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "用户",
        Locale::En => "User",
    }
}

pub fn msg_role_assistant(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "助手",
        Locale::En => "Assistant",
    }
}

pub fn msg_role_system(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "系统",
        Locale::En => "System",
    }
}

pub fn msg_role_other(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "其它",
        Locale::En => "Other",
    }
}

/// 分阶段规划 SSE `executor_kind` 蛇形值 → 短标签（时间线用）。
pub fn staged_executor_kind_short_label(l: Locale, kind: &str) -> String {
    match kind.trim() {
        "review_readonly" => match l {
            Locale::ZhHans => "只读审阅".to_string(),
            Locale::En => "read-only".to_string(),
        },
        "patch_write" => match l {
            Locale::ZhHans => "补丁修改".to_string(),
            Locale::En => "patch".to_string(),
        },
        "test_runner" => match l {
            Locale::ZhHans => "测试".to_string(),
            Locale::En => "tests".to_string(),
        },
        t if !t.is_empty() => match l {
            Locale::ZhHans => format!("角色 {t}"),
            Locale::En => format!("role {t}"),
        },
        _ => String::new(),
    }
}

fn staged_step_status_line(l: Locale, status: &str) -> String {
    match status {
        "ok" => match l {
            Locale::ZhHans => "完成".to_string(),
            Locale::En => "done".to_string(),
        },
        "failed" => match l {
            Locale::ZhHans => "失败".to_string(),
            Locale::En => "failed".to_string(),
        },
        "cancelled" => match l {
            Locale::ZhHans => "已取消".to_string(),
            Locale::En => "cancelled".to_string(),
        },
        _ => status.to_string(),
    }
}

/// 时间线旁注：分阶段单步开始（**不**进入模型上下文）。
pub fn timeline_staged_step_started(
    l: Locale,
    step_index: usize,
    total_steps: usize,
    description: &str,
    executor_kind: Option<&str>,
) -> String {
    const MAX_DESC: usize = 72;
    let mut d = description.trim().to_string();
    if d.chars().count() > MAX_DESC {
        d = d.chars().take(MAX_DESC).collect::<String>();
        d.push('…');
    }
    let role = executor_kind
        .map(|k| staged_executor_kind_short_label(l, k))
        .filter(|s| !s.is_empty());
    let role_sep = match l {
        Locale::ZhHans => " · ",
        Locale::En => " · ",
    };
    match l {
        Locale::ZhHans => {
            let mid = match &role {
                Some(r) => format!("{role_sep}{r}"),
                None => String::new(),
            };
            if d.is_empty() {
                format!("分阶段 · 第 {step_index}/{total_steps} 步{mid}")
            } else {
                format!("分阶段 · 第 {step_index}/{total_steps} 步{mid} · {d}")
            }
        }
        Locale::En => {
            let mid = match &role {
                Some(r) => format!("{role_sep}{r}"),
                None => String::new(),
            };
            if d.is_empty() {
                format!("Staged · step {step_index}/{total_steps}{mid}")
            } else {
                format!("Staged · step {step_index}/{total_steps}{mid} · {d}")
            }
        }
    }
}

/// 时间线旁注：分阶段单步结束。
pub fn timeline_staged_step_finished(
    l: Locale,
    step_index: usize,
    total_steps: usize,
    status: &str,
    executor_kind: Option<&str>,
) -> String {
    let st = staged_step_status_line(l, status);
    let role = executor_kind
        .map(|k| staged_executor_kind_short_label(l, k))
        .filter(|s| !s.is_empty());
    let tail = match &role {
        Some(r) => match l {
            Locale::ZhHans => format!(" · {r}"),
            Locale::En => format!(" · {r}"),
        },
        None => String::new(),
    };
    match l {
        Locale::ZhHans => format!("分阶段 · 第 {step_index}/{total_steps} 步 {st}{tail}"),
        Locale::En => format!("Staged · step {step_index}/{total_steps} {st}{tail}"),
    }
}

pub fn msg_tool_run_group_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "连续工具输出",
        Locale::En => "Consecutive tool output",
    }
}

pub fn msg_tool_run_count(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 条工具输出"),
        Locale::En => {
            if n == 1 {
                "1 tool output".to_string()
            } else {
                format!("{n} tool outputs")
            }
        }
    }
}

pub fn msg_tool_collapse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠连续工具输出",
        Locale::En => "Collapse tool outputs",
    }
}

pub fn msg_tool_collapse_aria(l: Locale) -> &'static str {
    msg_tool_collapse_title(l)
}

pub fn msg_tool_collapse_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠",
        Locale::En => "Collapse",
    }
}

pub fn msg_tool_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开连续工具输出",
        Locale::En => "Expand tool outputs",
    }
}

pub fn msg_tool_expand_aria(l: Locale) -> &'static str {
    msg_tool_expand_title(l)
}

pub fn msg_tool_expand_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开",
        Locale::En => "Expand",
    }
}

pub fn msg_jump_user_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "点击跳转到对应用户消息",
        Locale::En => "Jump to related user message",
    }
}

pub fn msg_jump_user_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跳转到对应用户消息",
        Locale::En => "Jump to user message",
    }
}

pub fn msg_select_label_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选中以加入导出",
        Locale::En => "Select for export",
    }
}

pub fn msg_select_cb_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选中此条以导出 Markdown",
        Locale::En => "Select for Markdown export",
    }
}

pub fn msg_actions_group_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "消息操作",
        Locale::En => "Message actions",
    }
}

pub fn msg_copy_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "复制本条展示文本",
        Locale::En => "Copy displayed text",
    }
}

pub fn msg_copy_aria(l: Locale) -> &'static str {
    msg_copy_title(l)
}

pub fn msg_regen_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息并重新生成（服务端会话需已持久化）",
        Locale::En => "Delete from here and regenerate (server session must be persisted)",
    }
}

pub fn msg_regen_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "从此处重试",
        Locale::En => "Regenerate from here",
    }
}

pub fn msg_branch_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息（不自动发送；服务端会话同步截断需已持久化）",
        Locale::En => "Branch: delete from here (no auto-send; server sync needs persistence)",
    }
}

pub fn msg_branch_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分支对话",
        Locale::En => "Branch conversation",
    }
}

pub fn msg_retry_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试当前助手生成",
        Locale::En => "Retry assistant generation",
    }
}

pub fn msg_retry_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试",
        Locale::En => "Retry",
    }
}

// --- 系统提示（alert / confirm）---

pub fn clipboard_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "复制失败：浏览器未授权剪贴板或不可用。",
        Locale::En => "Copy failed: clipboard permission denied or unavailable.",
    }
}

pub fn delete_session_confirm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "确定删除此本地会话？此操作不可恢复。",
        Locale::En => "Delete this local session? This cannot be undone.",
    }
}

/// 新建会话默认标题（写入 `ChatSession.title`）；与旧数据 **`新会话`** 等价判断见 [`is_default_session_title`]。
pub fn default_session_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新会话",
        Locale::En => "New chat",
    }
}

/// 是否与当前语言下的默认会话标题等价（含历史中文默认值）。
pub fn is_default_session_title(s: &str) -> bool {
    let t = s.trim();
    t == default_session_title(Locale::ZhHans)
        || t == default_session_title(Locale::En)
        || t == "新会话"
        || t.eq_ignore_ascii_case("new chat")
}

/// 侧栏/列表展示用：默认标题随界面语言切换，其它标题保持原样。
pub fn session_title_for_display(stored: &str, loc: Locale) -> String {
    if is_default_session_title(stored) {
        default_session_title(loc).to_string()
    } else {
        stored.to_string()
    }
}

// --- 聊天列空态 ---

pub fn chat_empty_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "开始对话",
        Locale::En => "Start a conversation",
    }
}

// --- 流式 / 停止 ---

pub fn stream_empty_reply(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复)",
        Locale::En => "(No reply)",
    }
}

pub fn stream_stopped_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "\n\n[已停止]",
        Locale::En => "\n\n[Stopped]",
    }
}

pub fn stream_stopped_inline(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已停止",
        Locale::En => "Stopped",
    }
}

pub fn chat_failed_banner(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "对话失败",
        Locale::En => "Chat failed",
    }
}

// --- 审批条 ---

pub fn approval_toggle_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "需要审批：运行命令",
        Locale::En => "Approval required: run command",
    }
}

pub fn approval_deny(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "拒绝",
        Locale::En => "Deny",
    }
}

pub fn approval_allow_once(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "允许一次",
        Locale::En => "Allow once",
    }
}

pub fn approval_allow_always(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "始终允许",
        Locale::En => "Always allow",
    }
}

pub fn ellipsis_tail() -> &'static str {
    "…"
}

// --- 状态栏 ---

pub fn status_fetch_error(l: Locale, err: &str) -> String {
    match l {
        Locale::ZhHans => format!("无法加载状态（/status）：{err}"),
        Locale::En => format!("Failed to load status (/status): {err}"),
    }
}

pub fn status_retry(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试",
        Locale::En => "Retry",
    }
}

pub fn status_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载状态",
        Locale::En => "Loading status",
    }
}

pub fn status_chip_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型",
        Locale::En => "Model",
    }
}

pub fn status_chip_base_url(_l: Locale) -> &'static str {
    "base_url"
}

pub fn status_role_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "角色",
        Locale::En => "Role",
    }
}

pub fn status_role_title_attr(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Agent 角色（对标 CLI /agent set）",
        Locale::En => "Agent role (same as CLI /agent set)",
    }
}

pub fn status_context_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文",
        Locale::En => "Context",
    }
}

pub fn status_context_title(l: Locale, used: usize, budget: usize, over_budget: bool) -> String {
    match l {
        Locale::ZhHans => {
            let hint = if over_budget {
                "已超过配置的字符预算，服务端可能在请求前裁剪历史。"
            } else {
                "与服务端注入/裁剪策略大致对照，非 token 精确值。"
            };
            format!(
                "本地估算：当前会话消息 + 输入草稿约 {used} 字符；context_char_budget={budget}。{hint}"
            )
        }
        Locale::En => {
            let hint = if over_budget {
                "Over the configured char budget; the server may trim older turns before the request."
            } else {
                "Rough local estimate vs server char budget—not exact tokens."
            };
            format!(
                "Local estimate: messages + composer draft ≈ {used} chars; context_char_budget={budget}. {hint}"
            )
        }
    }
}

pub fn status_context_title_no_budget(l: Locale, used: usize) -> String {
    match l {
        Locale::ZhHans => {
            format!("本地估算约 {used} 字符（context_char_budget=0，未启用按字符预算对照）")
        }
        Locale::En => format!(
            "Local estimate ≈ {used} chars (context_char_budget=0; no char budget configured)"
        ),
    }
}

pub fn status_context_meta_chars(l: Locale, used: usize) -> String {
    match l {
        Locale::ZhHans => format!("{used} 字"),
        Locale::En => format!("{used} ch"),
    }
}

pub fn status_context_meta_pct(l: Locale, pct: u32) -> String {
    match l {
        Locale::ZhHans => format!("{pct}%"),
        Locale::En => format!("{pct}%"),
    }
}

pub fn status_context_rev(l: Locale, rev: u64) -> String {
    match l {
        Locale::ZhHans => format!("rev {rev}"),
        Locale::En => format!("rev {rev}"),
    }
}

pub fn status_default_option(l: Locale, id: Option<&str>) -> String {
    match l {
        Locale::ZhHans => match id {
            Some(i) => format!("default ({i})"),
            None => "default".to_string(),
        },
        Locale::En => match id {
            Some(i) => format!("default ({i})"),
            None => "default".to_string(),
        },
    }
}

pub fn status_unavailable(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "/status 不可用",
        Locale::En => "/status unavailable",
    }
}

pub fn status_error_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "错误: ",
        Locale::En => "Error: ",
    }
}

pub fn status_tool_running(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具执行中…",
        Locale::En => "Running tools…",
    }
}

pub fn status_model_running(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型生成中…",
        Locale::En => "Model generating…",
    }
}

pub fn status_ready(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "就绪",
        Locale::En => "Ready",
    }
}

// --- 工作区树 ---

pub fn workspace_tree_no_data(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（无数据）",
        Locale::En => "(No data)",
    }
}

pub fn workspace_tree_toggle_dir_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开或折叠子目录",
        Locale::En => "Expand or collapse subdirectory",
    }
}

pub fn workspace_tree_expand_folder(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => format!("展开子文件夹 {name}"),
        Locale::En => format!("Expand subfolder {name}"),
    }
}

pub fn workspace_tree_collapse_folder(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => format!("折叠子文件夹 {name}"),
        Locale::En => format!("Collapse subfolder {name}"),
    }
}

// --- 助手 Markdown 折叠 ---

pub fn assistant_md_collapse(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起",
        Locale::En => "Collapse",
    }
}

pub fn assistant_md_expand_full(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开全文",
        Locale::En => "Expand full text",
    }
}

// --- message_format / 工具卡 ---

pub fn tool_card_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具：",
        Locale::En => "Tool: ",
    }
}

pub fn tool_card_fallback(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具输出",
        Locale::En => "Tool output",
    }
}

pub fn plan_generated(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已生成分阶段规划。",
        Locale::En => "Staged plan generated.",
    }
}

pub fn plan_step_no_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(未提供描述)",
        Locale::En => "(no description)",
    }
}

pub fn plan_step_placeholder_id() -> &'static str {
    "step"
}

pub fn plan_step_line(l: Locale, idx: usize, id: &str, desc: &str) -> String {
    let n = idx + 1;
    match l {
        Locale::ZhHans => format!("{n}. `{id}`: {desc}"),
        Locale::En => format!("{n}. `{id}`: {desc}"),
    }
}

// --- Markdown 导出（前端下载用，与 CLI 中文标题可并存）---

pub fn export_md_title_full(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "# CrabMate 聊天记录\n\n",
        Locale::En => "# CrabMate chat export\n\n",
    }
}

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

// --- API / 存储错误（设置、分支、审批等回显）---

pub fn api_err_no_local_storage(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无 localStorage",
        Locale::En => "localStorage is unavailable",
    }
}

pub fn api_err_write_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 api_base",
        Locale::En => "Could not save api_base",
    }
}

pub fn api_err_write_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 model",
        Locale::En => "Could not save model",
    }
}

pub fn api_err_write_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法写入 api_key",
        Locale::En => "Could not save api_key",
    }
}

pub fn api_err_workspace_set_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置失败",
        Locale::En => "Workspace update failed",
    }
}

pub fn api_err_request_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请求失败",
        Locale::En => "Request failed",
    }
}

pub fn api_err_no_response_body(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无响应体",
        Locale::En => "Empty response body",
    }
}

pub fn api_err_branch_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分支请求未成功",
        Locale::En => "Branch request did not succeed",
    }
}

pub fn api_err_approval_failed(l: Locale, status: u16) -> String {
    match l {
        Locale::ZhHans => format!("审批请求失败 {status}"),
        Locale::En => format!("Approval request failed ({status})"),
    }
}
