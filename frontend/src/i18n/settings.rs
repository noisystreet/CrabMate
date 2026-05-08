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

pub fn settings_intro(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "修改后需点击「保存全部」才会写入本机（localStorage）并生效。模型网关与 API 密钥也可仅存本机；发消息时会在 JSON 中附带覆盖项，请仅在可信环境（HTTPS）使用。"
        }
        Locale::En => {
            "Changes apply after you click “Save all”; they are written to localStorage. Model endpoint and API key can also stay in the browser; they are sent as JSON overrides with each message—use only on trusted connections (HTTPS)."
        }
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

pub fn settings_executor_llm_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行阶段使用的模型网关覆盖。留空则使用主模型设置或服务端默认配置。",
        Locale::En => {
            "Override for the model endpoint used during execution phase. Leave empty to use main model settings or server default."
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

pub fn settings_label_api_base_preset(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "常用网关",
        Locale::En => "Common providers",
    }
}

pub fn settings_label_executor_api_base(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器 API 基址（api_base）",
        Locale::En => "Executor API base (api_base)",
    }
}

pub fn settings_api_base_preset_server(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "使用服务端配置（不覆盖）",
        Locale::En => "Use server default (no override)",
    }
}

pub fn settings_api_base_preset_ollama(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Ollama（本机）",
        Locale::En => "Ollama (local)",
    }
}

pub fn settings_api_base_preset_deepseek(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "DeepSeek",
        Locale::En => "DeepSeek",
    }
}

pub fn settings_api_base_preset_minimax(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "MiniMax",
        Locale::En => "MiniMax",
    }
}

pub fn settings_api_base_preset_zhipu(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "智谱 GLM",
        Locale::En => "Zhipu GLM",
    }
}

pub fn settings_api_base_preset_moonshot(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Moonshot（Kimi）",
        Locale::En => "Moonshot (Kimi)",
    }
}

pub fn settings_api_base_preset_custom(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "自定义 URL…",
        Locale::En => "Custom URL…",
    }
}

pub fn settings_api_base_preset_label(id: &str, l: Locale) -> &'static str {
    match id {
        "server" => settings_api_base_preset_server(l),
        "ollama" => settings_api_base_preset_ollama(l),
        "deepseek" => settings_api_base_preset_deepseek(l),
        "minimax" => settings_api_base_preset_minimax(l),
        "zhipu" => settings_api_base_preset_zhipu(l),
        "moonshot" => settings_api_base_preset_moonshot(l),
        _ => settings_api_base_preset_custom(l),
    }
}

pub fn settings_label_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型名称（model）",
        Locale::En => "Model name (model)",
    }
}

pub fn settings_label_temperature(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "温度（temperature）",
        Locale::En => "Temperature (temperature)",
    }
}

pub fn settings_label_execution_mode(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行模式",
        Locale::En => "Execution mode",
    }
}

pub fn settings_execution_mode_rolling(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "滚动规划模式",
        Locale::En => "Rolling planning",
    }
}

pub fn settings_execution_mode_hierarchical(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分层执行模式",
        Locale::En => "Hierarchical execution",
    }
}

pub fn settings_execution_mode_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "滚动规划：每步执行后自动重规划。分层执行：由 Manager/Operator 分解并串行完成子目标。"
        }
        Locale::En => {
            "Rolling planning replans after each executed step. Hierarchical execution uses Manager/Operator sub-goal decomposition."
        }
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

pub fn settings_llm_thinking_mode_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "覆盖每轮发往模型的 thinking 相关字段（如智谱 GLM 深度思考、Moonshot kimi-k2.5 默认思考）。开启：发送 thinking enabled；关闭：智谱不写 thinking，Kimi k2.5 发送 disabled。仅影响本机保存的聊天请求覆盖。"
        }
        Locale::En => {
            "Per-request override for vendor thinking fields (e.g. Zhipu GLM deep thinking, Moonshot kimi-k2.5 default thinking). On: request enabled; Off: omit Zhipu thinking and send disabled for kimi-k2.5. Stored locally and sent with each chat request."
        }
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

pub fn settings_temperature_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "仅覆盖当前浏览器发起的聊天请求温度。",
        Locale::En => "Overrides temperature for chats started from this browser only.",
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

pub fn settings_llm_context_tokens_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "模型输入+输出的 token 上限（供应商计量）。用于推导会话裁剪近似字符预算；与配置 llm_context_tokens 等价。"
        }
        Locale::En => {
            "Vendor token budget for input+output; drives approximate session trimming (same as llm_context_tokens)."
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

pub fn settings_label_executor_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器模型名称（model）",
        Locale::En => "Executor model name (model)",
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

pub fn settings_label_executor_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器 API 密钥",
        Locale::En => "Executor API key",
    }
}

pub fn settings_ph_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "留空保留已存密钥；填写新密钥后点「保存全部」",
        Locale::En => "Leave blank to keep saved key; enter a new key, then Save all",
    }
}

pub fn settings_ph_executor_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "留空保留已存密钥；填写新密钥后点「保存全部」",
        Locale::En => "Leave blank to keep saved key; enter a new key, then Save all",
    }
}

pub fn settings_key_saved_note(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前已在本机保存密钥（不会回显到输入框）。",
        Locale::En => "A key is saved locally (not shown in the field).",
    }
}

pub fn settings_executor_key_saved_note(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器密钥已在本机保存。",
        Locale::En => "Executor key is saved locally.",
    }
}

pub fn settings_clear_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "清除已存密钥",
        Locale::En => "Clear saved key",
    }
}

pub fn settings_clear_executor_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "清除执行器密钥",
        Locale::En => "Clear executor key",
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

pub fn settings_section_appearance_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "配置语言、主题和页面背景效果。",
        Locale::En => "Configure language, theme, and page background effects.",
    }
}

pub fn settings_section_llm_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "主模型配置",
        Locale::En => "Primary Model",
    }
}

pub fn settings_section_llm_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "配置聊天阶段使用的模型网关、模型名与 API 密钥。",
        Locale::En => "Set endpoint, model name, and API key for chat phase.",
    }
}

pub fn settings_section_executor_llm_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "执行器模型配置",
        Locale::En => "Executor Model",
    }
}

pub fn settings_section_executor_llm_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "配置执行阶段的模型覆盖参数。",
        Locale::En => "Set model override options for execution phase.",
    }
}

pub fn settings_section_tools_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具",
        Locale::En => "Tools",
    }
}

pub fn settings_section_tools_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "与本机聊天请求相关的工具行为覆盖（仅存浏览器）。",
        Locale::En => "Tool-related overrides for chat requests (browser-only).",
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

pub fn settings_tools_readonly_ttl_cache_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "对应服务端只读类 run_command 的短时进程内缓存（readonly_tool_ttl_cache_secs）。勾选时不在请求中覆盖；取消勾选后每条 /chat/stream 会附带 readonly_tool_ttl_cache_secs: 0。"
        }
        Locale::En => {
            "Matches the server-side short-lived in-process cache for eligible read-only run_command calls. When checked, requests do not override it; when unchecked, each /chat/stream sends readonly_tool_ttl_cache_secs: 0."
        }
    }
}

pub fn settings_section_shortcuts_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "快捷键",
        Locale::En => "Keyboard Shortcuts",
    }
}

pub fn settings_section_shortcuts_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查看常用交互快捷键和焦点行为。",
        Locale::En => "View common shortcuts and focus behavior.",
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
