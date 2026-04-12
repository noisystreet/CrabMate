use super::Locale;

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

pub fn settings_label_api_base_preset(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "常用网关",
        Locale::En => "Common providers",
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
