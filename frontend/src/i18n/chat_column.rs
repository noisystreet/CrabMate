use super::Locale;

// --- 聊天列空态 / 输入区 ---

pub fn chat_tui_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "CrabMate 已就绪。输入消息开始纯文本流式对话。",
        Locale::En => "CrabMate ready. Send a message to start the plain-text stream.",
    }
}

pub fn chat_tui_tool_status_done(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "完成",
        Locale::En => "done",
    }
}

pub fn chat_tui_tool_status_failed(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "失败",
        Locale::En => "failed",
    }
}

pub fn composer_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "输入消息，Enter 发送 / Shift+Enter 换行；/ 选用技能；file:/// 或 @相对路径 展开文件…"
        }
        Locale::En => {
            "Message: Enter send / Shift+Enter newline; / for skills; file:/// or @rel/path expands file…"
        }
    }
}

/// 工作区树双击插入文件引用时路径含空白。
pub fn composer_ws_path_whitespace_err(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "该文件路径含空格，无法自动生成 file:/// 引用，请手动输入相对路径。",
        Locale::En => {
            "This path contains spaces; cannot auto-insert file:/// ref — type the relative path manually."
        }
    }
}

/// 侧栏工作区文件行：双击 / 拖放到输入框的提示。
pub fn workspace_tree_insert_file_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "双击或拖放到输入框：插入 file:///相对路径（发送时由服务端展开）",
        Locale::En => {
            "Double-click or drag onto composer to insert file:///relative-path (expanded server-side on send)"
        }
    }
}

/// 从系统文件管理器拖入非图片且无法映射到工作区路径。
pub fn composer_drop_need_workspace_tree(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "无法从系统拖放解析工作区路径；请从侧栏工作区树拖入文件，或双击插入 file:/// 引用。"
        }
        Locale::En => {
            "Cannot map OS drop to a workspace path — drag from the workspace tree, or double-click to insert a file:/// ref."
        }
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

pub fn composer_attach_image_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "附加图片",
        Locale::En => "Attach image",
    }
}

pub fn composer_slash_menu_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "斜杠命令与技能",
        Locale::En => "Slash commands and skills",
    }
}

pub fn composer_slash_menu_loading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载技能列表…",
        Locale::En => "Loading skills…",
    }
}

pub fn composer_slash_menu_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无匹配项（可试 `/skills`，或在 `.crabmate/skills` 添加技能）",
        Locale::En => "No matches (try `/skills`, or add under `.crabmate/skills`)",
    }
}

pub fn composer_slash_menu_skills_disabled(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "技能已关闭（skills_enabled=false）；仍可选用下方内建命令",
        Locale::En => "Skills disabled (skills_enabled=false); built-in commands still listed",
    }
}

pub fn composer_slash_section_commands(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "命令",
        Locale::En => "Commands",
    }
}

pub fn composer_slash_section_skills(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "技能",
        Locale::En => "Skills",
    }
}

pub fn composer_slash_builtin_skills(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查看当前工作区 skills 概览（不调用模型）",
        Locale::En => "List workspace skills overview (no model call)",
    }
}

/// 用户气泡内联：显式 `/<skill-id>` 的展示前缀（后接 skill id）。
pub fn msg_skill_invoke_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "使用",
        Locale::En => "Using",
    }
}

/// 用户气泡内联：显式 skill 的展示后缀（接在 id 后）。
pub fn msg_skill_invoke_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "skill",
        Locale::En => "skill",
    }
}

pub fn composer_slash_builtin_skills_list(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "列出可 `/id` 调用的技能明细",
        Locale::En => "List callable `/id` skills in detail",
    }
}

pub fn composer_slash_builtin_help(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "控制命令帮助（不调用模型）",
        Locale::En => "Control slash help (no model call)",
    }
}

pub fn composer_slash_builtin_workspace(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示或切换工作区",
        Locale::En => "Show or switch workspace",
    }
}

pub fn composer_slash_builtin_agent(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "列出或设置 Agent 角色",
        Locale::En => "List or set agent role",
    }
}

pub fn composer_slash_builtin_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示或设置模型（写 user-data）",
        Locale::En => "Show or set model (persists to user-data)",
    }
}

pub fn composer_slash_builtin_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示或设置 api_base",
        Locale::En => "Show or set api_base",
    }
}

pub fn composer_slash_builtin_export(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出当前会话（JSON / Markdown）",
        Locale::En => "Export current session (JSON / Markdown)",
    }
}

pub fn composer_slash_builtin_config(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "配置摘要或 /config reload",
        Locale::En => "Config summary or /config reload",
    }
}

pub fn composer_slash_builtin_context(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文 token / 字符预算",
        Locale::En => "Context token / char budget",
    }
}

pub fn composer_slash_builtin_clear(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "清空当前会话消息",
        Locale::En => "Clear current session messages",
    }
}

pub fn composer_slash_builtin_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "密钥 status/set/clear（不进对话）",
        Locale::En => "API key status/set/clear (never in chat)",
    }
}

pub fn composer_remove_image_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "移除图片",
        Locale::En => "Remove image",
    }
}

pub fn clarification_panel_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "需要补充信息",
        Locale::En => "More information needed",
    }
}

pub fn clarification_submit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "提交澄清",
        Locale::En => "Submit answers",
    }
}

pub fn clarification_dismiss(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭",
        Locale::En => "Dismiss",
    }
}

pub fn clarification_required_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（必填）",
        Locale::En => " (required)",
    }
}

pub fn clarification_missing_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写所有必填项",
        Locale::En => "Please fill all required fields",
    }
}

pub fn clarification_user_bubble_stub(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（已提交澄清问卷）",
        Locale::En => "(Clarification submitted)",
    }
}
// --- 聊天列空态 ---

pub fn chat_history_load_older(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载更早的消息",
        Locale::En => "Load older messages",
    }
}

pub fn chat_history_loading_older(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在加载更早的消息…",
        Locale::En => "Loading older messages…",
    }
}

pub fn debug_console_region_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "思维与工具调试台",
        Locale::En => "Thinking and tool debug console",
    }
}

pub fn debug_console_empty_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "发起流式对话后，推理增量与工具上下文摘要会出现在此（若服务端用 `CM_THINKING_TRACE_ENABLED=0` 关闭则不会有事件）。"
        }
        Locale::En => {
            "After a streamed reply, reasoning deltas and tool context summaries appear here (unless the server disabled traces with `CM_THINKING_TRACE_ENABLED=0`)."
        }
    }
}
