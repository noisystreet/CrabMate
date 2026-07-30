//! 设置页 MCP 相关文案。

use super::Locale;

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
            format!("已启用的服务器「{name}」须含 command 或 url，请用 JSON 导入或先禁用后再保存。")
        }
        Locale::En => format!(
            "Enabled server \"{name}\" needs a command or url; import via JSON or disable it before saving."
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
            "粘贴 MCP 配置 JSON（含 mcpServers 对象，可为整份 mcp.json 或其中一段）。会从 command、args、cwd、env 或 url、headers 解析并写入配置；启动明文不在本页显示，连接时见服务端日志（target: crabmate）。粘贴完整 JSON 后会自动解析；也可点「解析并添加」或保存时合并。支持 stdio（command）与远程 Streamable HTTP（url；非 HTTPS 仅 loopback）。"
        }
        Locale::En => {
            "Paste MCP config JSON (an object with mcpServers—often a full mcp.json file or that section). Builds stored servers from command/args/cwd/env or url/headers—secrets are not shown here; see server logs on connect (target: crabmate). A full JSON paste is parsed automatically; or use Parse & add, or Save to merge. Supports stdio (command) and remote Streamable HTTP (url; non-HTTPS only on loopback)."
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

pub fn settings_mcp_transport_stdio(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "传输：本地 stdio（command）",
        Locale::En => "Transport: local stdio (command)",
    }
}

pub fn settings_mcp_transport_remote(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "传输：远程 Streamable HTTP（url）",
        Locale::En => "Transport: remote Streamable HTTP (url)",
    }
}

pub fn settings_mcp_bearer_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Bearer 令牌",
        Locale::En => "Bearer token",
    }
}

pub fn settings_mcp_bearer_hint_set(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本机已保存 Bearer（不回显）。留空并保存可清除。",
        Locale::En => "Bearer is saved locally (never echoed). Save empty to clear.",
    }
}

pub fn settings_mcp_bearer_hint_unset(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可选。写入本机 secrets，探测/对话时作为 Authorization。",
        Locale::En => "Optional. Stored in local secrets as Authorization for probe/chat.",
    }
}

pub fn settings_mcp_bearer_save(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存 Bearer",
        Locale::En => "Save Bearer",
    }
}

pub fn settings_mcp_bearer_saved(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Bearer 已保存",
        Locale::En => "Bearer saved",
    }
}

pub fn settings_mcp_bearer_cleared(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Bearer 已清除",
        Locale::En => "Bearer cleared",
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
