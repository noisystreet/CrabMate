use super::Locale;

// --- 流式占位 / 非工具卡正文（工具卡 compact/detail 见 `crabmate_tool_card`）---

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

pub fn plan_no_new_tool_calls_note(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（本轮不调用新工具）",
        Locale::En => "(No new tool calls this turn)",
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

fn tool_kind_emoji_hashed(name: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "🛠️", "⚙️", "🔩", "🧰", "📎", "🗂️", "📌", "🧲", "🔑", "🪛", "⚗️", "🧪", "📡", "🛰️", "🗃️",
        "📇", "🔖", "🏷️", "🪄", "✨", "🔔", "📯", "🧿", "🔮", "🎲", "🧩", "🎁", "🧱", "🪢", "📮",
        "🧬", "🦾", "🖇️", "🗝️", "🪁", "🎪", "💠", "🔷", "🔶", "🟣", "🦀", "🐙", "🐳", "🐍", "🐹",
        "☕", "🔀",
    ];
    let mut h: u32 = 2_166_136_261;
    for b in name.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(16_777_619);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

pub fn tool_kind_emoji(name: &str) -> &'static str {
    let n = name.trim();
    if n.is_empty() {
        return "🔧";
    }
    match n {
        "run_command" => "⚡",
        "playbook_run_commands" => "📜",
        "read_file" => "📄",
        "read_binary_meta" => "💽",
        "extract_in_file" => "📤",
        "read_dir" => "📁",
        "list_tree" => "🌲",
        "glob_files" => "✴️",
        "file_exists" => "❔",
        "create_file" => "📝",
        "append_file" => "➕",
        "create_dir" => "📂",
        "modify_file" => "✏️",
        "search_replace" => "🔁",
        "apply_patch" => "🩹",
        "chmod_file" => "🛡️",
        "copy_file" => "📋",
        "move_file" => "🚚",
        "symlink_info" => "🔗",
        "delete_file" | "delete_dir" => "🗑️",
        "search_in_files" => "🔎",
        "codebase_semantic_search" => "🧭",
        "http_fetch" | "http_request" => "🌐",
        "web_search" => "🔍",
        "get_weather" => "🌤️",
        "calc" => "🧮",
        "date_calc" => "📆",
        "convert_units" => "↔️",
        "regex_test" => "🔣",
        "format_file" | "format_check_file" => "🎨",
        "get_current_time" => "🕐",
        "text_transform" => "🔤",
        "table_text" => "🔠",
        "text_diff" => "⚖️",
        "json_format" => "🗃️",
        "package_query" => "📦",
        "find_symbol" => "🔭",
        "find_references" => "📍",
        "call_graph_sketch" => "🕸️",
        "rust_file_outline" => "📑",
        "code_stats" => "📊",
        "dependency_graph" => "🪢",
        "coverage_report" => "📈",
        "hash_file" => "🔢",
        "port_check" => "🔌",
        "process_list" => "📟",
        "archive_pack" => "🗜️",
        "archive_unpack" => "📤",
        "archive_list" => "📃",
        "run_lints" | "quality_workspace" | "rust_backtrace_analyze" => "✅",
        "cargo_audit" => "🔒",
        "cargo_deny" => "🚧",
        "long_term_remember" => "💭",
        "long_term_forget" => "🧹",
        "long_term_memory_list" => "📚",
        "summarize_experience" => "📔",
        "add_reminder" | "complete_reminder" | "delete_reminder" => "⏰",
        "list_reminders" => "📒",
        "add_event" | "delete_event" | "update_event" => "🗓️",
        "list_events" => "📅",
        "diagnostic_summary" => "🩺",
        "error_output_playbook" => "📕",
        "present_clarification_questionnaire" => "❓",
        "changelog_draft" => "📰",
        "license_notice" => "🧾",
        "repo_overview_sweep" => "🗺️",
        "todo_scan" => "🗒️",
        "env_var_check" => "🔐",
        "structured_validate" => "✔️",
        "structured_query" => "🔬",
        "structured_diff" => "➖",
        "structured_patch" => "🧩",
        "markdown_check_links" => "📎",
        "workflow_execute" => "⚙️",
        "ci_pipeline_local" => "🛤️",
        "release_ready_check" => "🚀",
        "terminal_session" => "⌨️",
        _ => tool_kind_emoji_hashed(n),
    }
}
