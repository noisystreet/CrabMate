//! 工具卡展示用文案（zh-Hans / en）。

/// Web 界面语言（zh-Hans / en）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCardLocale {
    ZhHans,
    En,
}

impl ToolCardLocale {
    #[must_use]
    pub fn from_slug(slug: &str) -> Self {
        if slug.eq_ignore_ascii_case("en") {
            Self::En
        } else {
            Self::ZhHans
        }
    }
}

// --- message_format / 工具卡 ---

pub fn tool_card_prefix(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "工具：",
        ToolCardLocale::En => "Tool: ",
    }
}

pub fn tool_card_fallback(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "工具输出",
        ToolCardLocale::En => "Tool output",
    }
}

pub fn plan_generated(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "已生成分阶段规划。",
        ToolCardLocale::En => "Staged plan generated.",
    }
}

/// 紧凑规划 JSON（`no_new_tool_calls`）在正文中的附注。
pub fn plan_no_new_tool_calls_note(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "（本轮不调用新工具）",
        ToolCardLocale::En => "(No new tool calls this turn)",
    }
}

pub fn plan_step_no_desc(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "(未提供描述)",
        ToolCardLocale::En => "(no description)",
    }
}

pub fn plan_step_placeholder_id() -> &'static str {
    "step"
}

pub fn plan_step_line(l: ToolCardLocale, idx: usize, id: &str, desc: &str) -> String {
    let n = idx + 1;
    match l {
        ToolCardLocale::ZhHans => format!("{n}. `{id}`: {desc}"),
        ToolCardLocale::En => format!("{n}. `{id}`: {desc}"),
    }
}

// --- `message_format/tool_card.rs` 展示层（与 Rust 工具中文摘要解析配套）---

/// 稳定哈希：同名工具图标不变，不同名在调色板中大多可区分（MCP、长尾 `cargo_*` / `git_*` 等）。
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

/// 按内置工具 `name`（蛇形）选气泡左侧图标；空名回退 🔧，未单独列出的名走 [`tool_kind_emoji_hashed`]。
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

pub fn tool_human_name(l: ToolCardLocale, name: &str) -> String {
    match (l, name) {
        (ToolCardLocale::ZhHans, "run_command") => "命令执行".to_string(),
        (ToolCardLocale::ZhHans, "read_file") => "读取文件".to_string(),
        (ToolCardLocale::ZhHans, "create_file") => "创建文件".to_string(),
        (ToolCardLocale::ZhHans, "read_dir") => "读取目录".to_string(),
        (ToolCardLocale::ZhHans, "search_in_files") => "全文检索".to_string(),
        (ToolCardLocale::ZhHans, "list_files") => "列出文件".to_string(),
        (ToolCardLocale::ZhHans, "archive_unpack") => "解压缩".to_string(),
        (ToolCardLocale::ZhHans, "terminal_session") => "交互终端".to_string(),
        (ToolCardLocale::En, "run_command") => "Command run".to_string(),
        (ToolCardLocale::En, "read_file") => "Read file".to_string(),
        (ToolCardLocale::En, "create_file") => "Create file".to_string(),
        (ToolCardLocale::En, "read_dir") => "Read directory".to_string(),
        (ToolCardLocale::En, "search_in_files") => "Search files".to_string(),
        (ToolCardLocale::En, "list_files") => "List files".to_string(),
        (ToolCardLocale::En, "archive_unpack") => "Unpack archive".to_string(),
        (ToolCardLocale::En, "terminal_session") => "Terminal".to_string(),
        _ => name.to_string(),
    }
}

/// 将工具摘要首行里「空格 + 状态」规范为换行，便于分行解析（服务端多为中文标签）。
pub fn tool_summary_normalize_line_breaks(sum: &str, loc: ToolCardLocale) -> String {
    match loc {
        ToolCardLocale::ZhHans => sum
            .replace(" 退出码：", "\n退出码：")
            .replace(" 标准输出：", "\n标准输出：")
            .replace(" 标准错误：", "\n标准错误："),
        ToolCardLocale::En => sum
            .replace(" exit code:", "\nexit code:")
            .replace(" stdout:", "\nstdout:")
            .replace(" stderr:", "\nstderr:")
            // 仍可能收到中文格式工具输出
            .replace(" 退出码：", "\n退出码：")
            .replace(" 标准输出：", "\n标准输出：")
            .replace(" 标准错误：", "\n标准错误："),
    }
}

pub fn tool_cmd_success_sep(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => " 成功:",
        ToolCardLocale::En => " success:",
    }
}

pub fn tool_cmd_fail_sep(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => " 失败:",
        ToolCardLocale::En => " failed:",
    }
}

pub fn tool_exit_line_zero(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "退出码：0",
        ToolCardLocale::En => "exit code: 0",
    }
}

pub fn tool_line_stdout_prefix(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "标准输出：",
        ToolCardLocale::En => "stdout: ",
    }
}

pub fn tool_line_stderr_prefix(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "标准错误：",
        ToolCardLocale::En => "stderr: ",
    }
}

pub fn tool_line_exit_prefix(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "退出码：",
        ToolCardLocale::En => "exit code: ",
    }
}

pub fn tool_summary_label_stdout(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "输出",
        ToolCardLocale::En => "Output",
    }
}

pub fn tool_summary_label_stderr(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "错误输出",
        ToolCardLocale::En => "Stderr",
    }
}

/// 紧凑条左侧标题：成功态只展示工具人类名，避免与右侧摘要里的「已完成」等重复「完成」。
pub fn tool_title_completed(_l: ToolCardLocale, human: &str) -> String {
    human.to_string()
}

pub fn tool_title_failed(l: ToolCardLocale, human: &str) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("{human}失败"),
        ToolCardLocale::En => format!("{human} failed"),
    }
}

/// 重写旧版摘要首行时的成功标题：仅保留工具人类名，不附加「已完成」等状态词。
pub fn tool_rewrite_title_done(_l: ToolCardLocale, human: &str) -> String {
    human.to_string()
}

pub fn tool_rewrite_title_failed_run(l: ToolCardLocale, human: &str) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("{human} 执行失败"),
        ToolCardLocale::En => format!("{human} failed"),
    }
}

pub fn tool_read_dir_label_dir(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "目录：",
        ToolCardLocale::En => "dir:",
    }
}

pub fn tool_read_dir_label_shown(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "展示：",
        ToolCardLocale::En => "shown:",
    }
}

pub fn tool_read_dir_compact_entries(l: ToolCardLocale, n: usize) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("{n} 项"),
        ToolCardLocale::En => format!("{n} entries"),
    }
}

/// 紧凑条：全文检索时与「搜索 · 模式 · 范围」摘要左侧对齐。
pub fn tool_search_compact_header_label(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "搜索",
        ToolCardLocale::En => "Search",
    }
}

pub fn tool_read_file_label_path(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "路径：",
        ToolCardLocale::En => "path:",
    }
}

pub fn tool_read_file_label_lines(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "行数：",
        ToolCardLocale::En => "lines:",
    }
}

pub fn tool_read_file_default_path(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "文件",
        ToolCardLocale::En => "file",
    }
}

pub fn tool_read_file_lines_suffix(l: ToolCardLocale, n: usize) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("{n} 行"),
        ToolCardLocale::En => format!("{n} lines"),
    }
}

pub fn tool_search_label_keyword(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "关键词：",
        ToolCardLocale::En => "pattern:",
    }
}

pub fn tool_search_label_hits(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "命中：",
        ToolCardLocale::En => "hits:",
    }
}

pub fn tool_search_default_keyword(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "关键词",
        ToolCardLocale::En => "keyword",
    }
}

pub fn tool_search_compact_keyword_word(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "关键词",
        ToolCardLocale::En => "keyword",
    }
}

pub fn tool_search_compact_hits_suffix(l: ToolCardLocale, n: usize) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("命中 {n} 处"),
        ToolCardLocale::En => format!("{n} hits"),
    }
}

pub fn summary_line_looks_like_compact_signal(summary: &str, loc: ToolCardLocale) -> bool {
    summary.lines().any(|line| {
        let zh = line.contains("输出：") || line.contains("错误输出：") || line.contains("目录：");
        let en = line.contains("Output：") || line.contains("Stderr：") || line.contains("dir ");
        match loc {
            ToolCardLocale::ZhHans => zh,
            ToolCardLocale::En => en || zh,
        }
    })
}

pub fn tool_failure_suggest_timeout(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "建议：缩小命令范围后重试，或提高超时阈值。",
        ToolCardLocale::En => "Suggestion: retry with narrower scope or increase timeout.",
    }
}

pub fn tool_failure_suggest_invalid_args(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "建议：检查命令参数格式与路径是否正确。",
        ToolCardLocale::En => "Suggestion: verify command args format and paths.",
    }
}

pub fn tool_failure_suggest_generic(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "建议：检查错误输出并按需重试。",
        ToolCardLocale::En => "Suggestion: inspect stderr and retry if needed.",
    }
}

pub fn tool_failure_returned_code(l: ToolCardLocale, code: &str) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("工具返回错误码：{code}"),
        ToolCardLocale::En => format!("Tool returned error code: {code}"),
    }
}

pub fn tool_failure_no_detail(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "工具执行失败。",
        ToolCardLocale::En => "Tool execution failed.",
    }
}

pub fn tool_failure_impact_exit(l: ToolCardLocale, ec: i64) -> String {
    match l {
        ToolCardLocale::ZhHans => format!("当前步骤中断（退出码：{ec}），本轮后续动作可能被跳过。"),
        ToolCardLocale::En => {
            format!("This step stopped (exit code: {ec}); follow-up actions may be skipped.")
        }
    }
}

pub fn tool_failure_impact_no_exit(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "当前步骤中断，本轮后续动作可能被跳过。",
        ToolCardLocale::En => "This step stopped; follow-up actions may be skipped.",
    }
}

pub fn tool_failure_suggest_fallback(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "检查错误输出并按需重试。",
        ToolCardLocale::En => "Inspect stderr and retry if needed.",
    }
}

pub fn tool_card_compact_skip_headings(l: ToolCardLocale) -> [&'static str; 3] {
    match l {
        ToolCardLocale::ZhHans => ["完成了什么", "产出是什么", "可继续做什么"],
        ToolCardLocale::En => ["What was done", "What was produced", "What next"],
    }
}

/// 工具卡展开详情中「完整原文输出」小标题（置于服务端下发的 `output` 正文前）。
pub fn tool_detail_full_output_heading(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "完整输出",
        ToolCardLocale::En => "Full output",
    }
}

/// 工具卡详情：写操作 Web diff 预览区块标题（来自 SSE `structured_preview.workspace_write_diff`）。
pub fn tool_workspace_write_diff_heading(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "变更预览（unified diff）",
        ToolCardLocale::En => "Change preview (unified diff)",
    }
}

pub fn tool_workspace_write_diff_truncated_note(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "（尚有文件或 diff 因体积上限未完整展示）",
        ToolCardLocale::En => "(Some files or diff content omitted due to size limits)",
    }
}

pub fn diag_error_what_title(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "发生了什么",
        ToolCardLocale::En => "What happened",
    }
}

pub fn diag_error_impact_title(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "影响范围",
        ToolCardLocale::En => "Impact",
    }
}

pub fn diag_error_next_title(l: ToolCardLocale) -> &'static str {
    match l {
        ToolCardLocale::ZhHans => "建议下一步",
        ToolCardLocale::En => "Next step",
    }
}

pub fn format_error_three_part(
    l: ToolCardLocale,
    happened: &str,
    impact: &str,
    suggestion: &str,
) -> String {
    format!(
        "{}\n{happened}\n\n{}\n{impact}\n\n{}\n{suggestion}",
        diag_error_what_title(l),
        diag_error_impact_title(l),
        diag_error_next_title(l),
    )
}
