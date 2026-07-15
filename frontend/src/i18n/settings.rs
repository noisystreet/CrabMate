use super::Locale;

// --- 设置弹窗 ---

pub fn settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置",
        Locale::En => "Settings",
    }
}

pub fn settings_nav_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置分区导航",
        Locale::En => "Settings sections",
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

pub fn settings_save_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存全部",
        Locale::En => "Save all",
    }
}

pub fn settings_discard_changes(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "放弃更改",
        Locale::En => "Discard",
    }
}

pub fn settings_unsaved_badge(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "未保存",
        Locale::En => "Unsaved",
    }
}

pub fn settings_save_all_ok(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已保存全部设置",
        Locale::En => "All settings saved",
    }
}

pub fn settings_nothing_to_save(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "没有需要保存的更改",
        Locale::En => "No changes to save",
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

pub fn settings_theme_material(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Material",
        Locale::En => "Material",
    }
}

pub fn settings_theme_high_contrast(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "高对比度",
        Locale::En => "High contrast",
    }
}

pub fn settings_theme_preset_label(l: Locale, slug: &str) -> &'static str {
    match slug {
        "dark" => settings_theme_dark(l),
        "light" => settings_theme_light(l),
        "material" => settings_theme_material(l),
        "high-contrast" => settings_theme_high_contrast(l),
        _ => settings_theme_dark(l),
    }
}

pub fn settings_label_theme_preset(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "配色方案",
        Locale::En => "Color scheme",
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

pub fn settings_block_executor_llm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器模型网关（可选覆盖）",
        Locale::En => "Executor model endpoint (optional override)",
    }
}

pub fn settings_label_saved_model_pick(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已保存模型",
        Locale::En => "Saved model",
    }
}

pub fn settings_saved_model_pick_placeholder(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请选择列表中的一项…",
        Locale::En => "Pick an entry…",
    }
}

pub fn settings_label_temperature(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "温度（temperature）",
        Locale::En => "Temperature (temperature)",
    }
}

pub fn settings_label_llm_thinking_mode(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型思考模式（thinking）",
        Locale::En => "Model thinking mode (thinking)",
    }
}

pub fn settings_thinking_mode_server(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跟随服务端配置",
        Locale::En => "Follow server config",
    }
}

pub fn settings_thinking_mode_on(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "开启（请求启用 thinking）",
        Locale::En => "On (request thinking enabled)",
    }
}

pub fn settings_thinking_mode_off(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭（请求禁用 thinking）",
        Locale::En => "Off (request thinking disabled)",
    }
}

pub fn settings_err_thinking_mode_invalid(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "思考模式须为 server、on 或 off",
        Locale::En => "Thinking mode must be server, on, or off",
    }
}

pub fn settings_ph_temperature(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "0 到 2，留空使用服务端默认值",
        Locale::En => "0 to 2, leave empty for server default",
    }
}

pub fn settings_label_llm_context_tokens(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文窗口（tokens）",
        Locale::En => "Context window (tokens)",
    }
}

pub fn settings_ph_llm_context_tokens(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "留空使用服务端；与 llm_context_tokens / CM_LLM_CONTEXT_TOKENS 一致",
        Locale::En => {
            "Leave empty for server default; same as llm_context_tokens / CM_LLM_CONTEXT_TOKENS"
        }
    }
}

pub fn settings_err_context_tokens_invalid(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文窗口须为非负整数",
        Locale::En => "Context window must be a non-negative integer",
    }
}

pub fn settings_err_context_tokens_range(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文窗口过大（上限 10000000）",
        Locale::En => "Context window too large (max 10000000)",
    }
}

pub fn settings_err_temperature_invalid(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "温度格式无效，请输入数字（例如 0.7）",
        Locale::En => "Invalid temperature format; enter a number (for example 0.7)",
    }
}

pub fn settings_err_temperature_range(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "温度必须在 0 到 2 之间",
        Locale::En => "Temperature must be between 0 and 2",
    }
}

// --- 设置页面 ---

pub fn settings_back(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "返回",
        Locale::En => "Back",
    }
}

pub fn settings_block_shortcuts(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "键盘",
        Locale::En => "Keyboard",
    }
}

pub fn settings_section_appearance_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "外观与界面",
        Locale::En => "Appearance & Interface",
    }
}

pub fn settings_section_appearance_desc(_l: Locale) -> &'static str {
    ""
}

pub fn settings_section_llm_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "主模型配置",
        Locale::En => "Primary Model",
    }
}

pub fn settings_section_llm_desc(_l: Locale) -> &'static str {
    ""
}

pub fn settings_section_executor_llm_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器模型配置",
        Locale::En => "Executor Model",
    }
}

pub fn settings_section_executor_llm_desc(_l: Locale) -> &'static str {
    ""
}

pub fn settings_section_tools_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具",
        Locale::En => "Tools",
    }
}

pub fn settings_section_tools_desc(_l: Locale) -> &'static str {
    ""
}

pub fn settings_block_session_storage(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话存储",
        Locale::En => "Session storage",
    }
}

pub fn settings_block_session_typography(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "界面与聊天字体",
        Locale::En => "UI & chat fonts",
    }
}

pub fn settings_session_typography_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "以下选项仅存于本机浏览器并立即生效；代码块与行内代码仍使用主题的等宽字体。"
        }
        Locale::En => {
            "These choices are stored in this browser only and apply immediately; code blocks still use the theme monospace stack."
        }
    }
}

pub fn settings_session_ui_font_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "界面字体（侧栏、顶栏、设置等）",
        Locale::En => "UI font (sidebar, chrome, settings)",
    }
}

pub fn settings_session_chat_font_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "聊天消息与输入框字体",
        Locale::En => "Chat message & composer font",
    }
}

pub fn settings_session_font_slug_label(l: Locale, slug: &str) -> &'static str {
    match slug {
        "default" => match l {
            Locale::ZhHans => "主题默认",
            Locale::En => "Theme default",
        },
        "dm_sans" => "DM Sans",
        "system" => match l {
            Locale::ZhHans => "系统无衬线",
            Locale::En => "System UI sans-serif",
        },
        "roboto" => "Roboto",
        "serif" => match l {
            Locale::ZhHans => "衬线体",
            Locale::En => "Serif",
        },
        "jetbrains" => "JetBrains Mono",
        "mono_system" => match l {
            Locale::ZhHans => "系统等宽",
            Locale::En => "System monospace",
        },
        _ => match l {
            Locale::ZhHans => "主题默认",
            Locale::En => "Theme default",
        },
    }
}

pub fn settings_section_session_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话",
        Locale::En => "Session",
    }
}

pub fn settings_section_session_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "控制当前浏览器连接到的服务端进程是否把 Web 会话写入 SQLite（仅本进程；重启 serve 后仍以配置文件为准）。界面与聊天字体可在下方单独设置，仅存本机并即时生效。"
        }
        Locale::En => {
            "Control whether this server process persists Web chat to SQLite (this process only; restart serve still follows config files). UI and chat fonts below are browser-only and apply immediately."
        }
    }
}

pub fn settings_session_sqlite_toggle_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "使用 SQLite 保存会话（当前进程）",
        Locale::En => "Persist sessions with SQLite (this process)",
    }
}

pub fn settings_session_sqlite_unconfigured_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "服务端未配置 conversation_store_sqlite_path，无法启用 SQLite；请在配置中设置后重启 serve。"
        }
        Locale::En => {
            "Server has no conversation_store_sqlite_path; set it in config and restart serve to enable SQLite."
        }
    }
}

pub fn settings_session_switch_busy(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在切换…",
        Locale::En => "Switching…",
    }
}

pub fn settings_tools_readonly_ttl_block_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "只读命令结果缓存",
        Locale::En => "Read-only command result cache",
    }
}

pub fn settings_tools_readonly_ttl_cache_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跟随服务端配置的 TTL 缓存（关闭则每条消息禁用该缓存）",
        Locale::En => {
            "Follow server TTL for this cache (when off, each message disables the cache)"
        }
    }
}

pub fn settings_section_shortcuts_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "快捷键",
        Locale::En => "Keyboard Shortcuts",
    }
}

pub fn settings_section_shortcuts_desc(_l: Locale) -> &'static str {
    ""
}

pub fn settings_section_mcp_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "MCP 服务器",
        Locale::En => "MCP Servers",
    }
}

pub fn settings_section_mcp_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "配置 stdio MCP 子进程（仅存本机 user-data）。command 等效于允许启动任意程序，请仅在信任环境下使用。"
        }
        Locale::En => {
            "Configure stdio MCP child processes (stored in local user-data only). A command can start arbitrary programs—use only in trusted environments."
        }
    }
}

pub fn settings_mcp_global_enabled_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "启用 MCP 工具合并",
        Locale::En => "Enable MCP tool merging",
    }
}

pub fn settings_mcp_timeout_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具调用超时（秒）",
        Locale::En => "Tool call timeout (seconds)",
    }
}

pub fn settings_mcp_servers_via_json_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请通过上方 JSON 导入添加 MCP 服务器；无法在此页手填启动命令。",
        Locale::En => {
            "Add MCP servers via JSON import above; start commands cannot be edited on this page."
        }
    }
}

pub fn settings_mcp_enabled_missing_command(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => {
            format!("已启用的服务器「{name}」缺少启动命令，请用 JSON 导入或先禁用后再保存。")
        }
        Locale::En => format!(
            "Enabled server \"{name}\" has no start command; import via JSON or disable it before saving."
        ),
    }
}

pub fn settings_mcp_import_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "从 MCP JSON 导入",
        Locale::En => "Import from MCP JSON",
    }
}

pub fn settings_mcp_import_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "粘贴 MCP 配置 JSON（含 mcpServers 对象，可为整份 mcp.json 或其中一段）。会从 command、args、cwd、env 解析服务器名称并写入配置；启动命令不在本页显示，连接时见服务端日志（target: crabmate）。粘贴完整 JSON 后会自动解析；也可点「解析并添加」或保存时合并。仅 stdio（有 command）；纯 url 远程 MCP 会跳过。"
        }
        Locale::En => {
            "Paste MCP config JSON (an object with mcpServers—often a full mcp.json file or that section). Server names and stored start commands are built from command, args, cwd, and env—the command is not shown here; see server logs on connect (target: crabmate). A full JSON paste is parsed automatically; or use Parse & add, or Save to merge. Stdio only (command required); url-only remote servers are skipped."
        }
    }
}

pub fn settings_mcp_import_placeholder(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "{\n  \"mcpServers\": {\n    \"my-server\": {\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"mcp-server\"]\n    }\n  }\n}"
        }
        Locale::En => {
            "{\n  \"mcpServers\": {\n    \"my-server\": {\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"mcp-server\"]\n    }\n  }\n}"
        }
    }
}

pub fn settings_mcp_import_auto_paste(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（已根据粘贴的 JSON 解析服务器；启动命令已写入配置）",
        Locale::En => "(Servers parsed from pasted JSON; start command stored in config)",
    }
}

pub fn settings_mcp_import_apply(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "解析并添加到列表",
        Locale::En => "Parse and add to list",
    }
}

pub fn settings_mcp_import_merged_on_save(l: Locale) -> String {
    match l {
        Locale::ZhHans => "已合并粘贴的 MCP JSON，正在保存…",
        Locale::En => "Merged pasted MCP JSON; saving…",
    }
    .to_string()
}

pub fn settings_mcp_import_success(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("已导入 {n} 个 MCP 服务器（启动命令已写入本机配置）"),
        Locale::En => format!("Imported {n} MCP server(s) (start commands saved locally)"),
    }
}

pub fn settings_mcp_import_skipped_remote(l: Locale, names: &str) -> String {
    match l {
        Locale::ZhHans => format!("已跳过远程 MCP（仅支持 stdio）：{names}"),
        Locale::En => format!("Skipped remote MCP (stdio only): {names}"),
    }
}

pub fn settings_mcp_save(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存 MCP 配置",
        Locale::En => "Save MCP settings",
    }
}

pub fn settings_mcp_probe(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "探测",
        Locale::En => "Probe",
    }
}

pub fn settings_mcp_probe_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "探测全部",
        Locale::En => "Probe all",
    }
}

pub fn settings_mcp_remove(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除",
        Locale::En => "Remove",
    }
}

pub fn settings_mcp_name_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "名称",
        Locale::En => "Name",
    }
}

pub fn settings_mcp_enabled_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "启用",
        Locale::En => "Enabled",
    }
}

pub fn settings_mcp_tools_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可用工具",
        Locale::En => "Available tools",
    }
}

pub fn settings_mcp_tools_col_name(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具名称",
        Locale::En => "Tool name",
    }
}

pub fn settings_mcp_tools_col_description(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "说明",
        Locale::En => "Description",
    }
}

pub fn settings_mcp_tools_desc_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "—",
        Locale::En => "—",
    }
}

pub fn settings_mcp_tools_probing(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在连接并拉取工具列表…",
        Locale::En => "Connecting and fetching tools…",
    }
}

pub fn settings_mcp_tools_none(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已连接，但远端未返回任何工具。",
        Locale::En => "Connected, but the server returned no tools.",
    }
}

pub fn settings_mcp_tools_probe_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存配置后将自动探测；也可点「探测」或「全部探测」。",
        Locale::En => "Tools appear after Save (auto-probe), or use Probe / Probe all.",
    }
}

pub fn settings_mcp_tools_server_disabled(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "该服务器未启用，不会在对话中合并工具。",
        Locale::En => "This server is disabled; its tools are not merged into chat.",
    }
}

pub fn settings_mcp_tools_toggle_label(l: Locale, tool_count: Option<usize>) -> String {
    let base = settings_mcp_tools_title(l);
    match tool_count {
        Some(n) => match l {
            Locale::ZhHans => format!("{base}（{n}）"),
            Locale::En => format!("{base} ({n})"),
        },
        None => base.to_string(),
    }
}

pub fn settings_mcp_tools_expand_aria(l: Locale, expanded: bool) -> &'static str {
    match (l, expanded) {
        (Locale::ZhHans, true) => "折叠可用工具列表",
        (Locale::ZhHans, false) => "展开可用工具列表",
        (Locale::En, true) => "Collapse available tools",
        (Locale::En, false) => "Expand available tools",
    }
}

pub fn settings_mcp_disconnected(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "未连接",
        Locale::En => "Not connected",
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

pub fn settings_saved_models_block_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已保存模型",
        Locale::En => "Saved models",
    }
}

pub fn settings_saved_models_remove(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "移除",
        Locale::En => "Remove",
    }
}

pub fn settings_models_delete_confirm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "确定移除这条已保存模型吗？",
        Locale::En => "Remove this saved model preset?",
    }
}

/// 行内「确认移除」按钮（替代部分环境下无响应的 `window.confirm`）。
pub fn settings_models_delete_apply_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "确认移除",
        Locale::En => "Confirm remove",
    }
}

pub fn settings_models_presets_persist_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无法保存已保存模型列表（本机存储不可用或写入失败）。",
        Locale::En => {
            "Could not save the saved model list (local storage unavailable or write failed)."
        }
    }
}

pub fn settings_models_add_open_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开添加已保存模型对话框",
        Locale::En => "Open dialog to add a saved model",
    }
}

pub fn settings_models_add_dialog_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "添加已保存模型",
        Locale::En => "Add saved model",
    }
}

pub fn settings_models_edit_dialog_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑已保存模型",
        Locale::En => "Edit saved model",
    }
}

pub fn settings_models_edit_submit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存修改",
        Locale::En => "Save changes",
    }
}

pub fn settings_models_row_edit_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑",
        Locale::En => "Edit",
    }
}

pub fn settings_models_row_edit_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑此模型预设",
        Locale::En => "Edit this saved model preset",
    }
}

pub fn settings_models_row_enabled_short(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "启用",
        Locale::En => "On",
    }
}

pub fn settings_models_row_enabled_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "启用或禁用此模型（禁用后不可选为当前主/执行器模型）",
        Locale::En => "Enable or disable this model (disabled models cannot be selected)",
    }
}

pub fn settings_models_preset_disabled_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（已禁用）",
        Locale::En => " (disabled)",
    }
}

pub fn settings_models_label_name(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示名称",
        Locale::En => "Display name",
    }
}

pub fn settings_models_label_base_url(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API Base URL",
        Locale::En => "API base URL",
    }
}

pub fn settings_models_label_model_id(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型 ID",
        Locale::En => "Model ID",
    }
}

pub fn settings_models_ph_model_id(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "留空则与显示名称相同",
        Locale::En => "Leave empty to match display name",
    }
}

pub fn settings_models_label_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "API 密钥（可选，仅存本机列表）",
        Locale::En => "API key (optional, stored in local list only)",
    }
}

pub fn settings_models_label_context_tokens(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文 token（可选）",
        Locale::En => "Context tokens (optional)",
    }
}

pub fn settings_models_validation_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写 Base URL 与显示名称。",
        Locale::En => "Please fill in base URL and display name.",
    }
}

pub fn settings_models_add_submit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加入列表",
        Locale::En => "Add to list",
    }
}

pub fn settings_models_cancel_form(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "取消",
        Locale::En => "Cancel",
    }
}

pub fn settings_models_ctx_line(l: Locale, ctx: &str) -> String {
    match l {
        Locale::ZhHans => format!("上下文 {ctx}"),
        Locale::En => format!("Context {ctx}"),
    }
}
