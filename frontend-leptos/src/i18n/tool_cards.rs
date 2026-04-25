use super::Locale;

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

// --- `message_format/tool_card.rs` 展示层（与 Rust 工具中文摘要解析配套）---

pub fn tool_human_name(l: Locale, name: &str) -> String {
    match (l, name) {
        (Locale::ZhHans, "run_command") => "命令执行".to_string(),
        (Locale::ZhHans, "read_file") => "读取文件".to_string(),
        (Locale::ZhHans, "read_dir") => "读取目录".to_string(),
        (Locale::ZhHans, "search_in_files") => "全文检索".to_string(),
        (Locale::ZhHans, "list_files") => "列出文件".to_string(),
        (Locale::En, "run_command") => "Command run".to_string(),
        (Locale::En, "read_file") => "Read file".to_string(),
        (Locale::En, "read_dir") => "Read directory".to_string(),
        (Locale::En, "search_in_files") => "Search files".to_string(),
        (Locale::En, "list_files") => "List files".to_string(),
        _ => name.to_string(),
    }
}

/// 将工具摘要首行里「空格 + 状态」规范为换行，便于分行解析（服务端多为中文标签）。
pub fn tool_summary_normalize_line_breaks(sum: &str, loc: Locale) -> String {
    match loc {
        Locale::ZhHans => sum
            .replace(" 退出码：", "\n退出码：")
            .replace(" 标准输出：", "\n标准输出：")
            .replace(" 标准错误：", "\n标准错误："),
        Locale::En => sum
            .replace(" exit code:", "\nexit code:")
            .replace(" stdout:", "\nstdout:")
            .replace(" stderr:", "\nstderr:")
            // 仍可能收到中文格式工具输出
            .replace(" 退出码：", "\n退出码：")
            .replace(" 标准输出：", "\n标准输出：")
            .replace(" 标准错误：", "\n标准错误："),
    }
}

pub fn tool_cmd_success_sep(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => " 成功:",
        Locale::En => " success:",
    }
}

pub fn tool_cmd_fail_sep(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => " 失败:",
        Locale::En => " failed:",
    }
}

pub fn tool_exit_line_zero(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "退出码：0",
        Locale::En => "exit code: 0",
    }
}

pub fn tool_line_stdout_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "标准输出：",
        Locale::En => "stdout: ",
    }
}

pub fn tool_line_stderr_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "标准错误：",
        Locale::En => "stderr: ",
    }
}

pub fn tool_line_exit_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "退出码：",
        Locale::En => "exit code: ",
    }
}

pub fn tool_summary_label_stdout(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "输出",
        Locale::En => "Output",
    }
}

pub fn tool_summary_label_stderr(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "错误输出",
        Locale::En => "Stderr",
    }
}

pub fn tool_title_completed(l: Locale, human: &str) -> String {
    match l {
        Locale::ZhHans => format!("{human}完成"),
        Locale::En => format!("{human} done"),
    }
}

pub fn tool_title_failed(l: Locale, human: &str) -> String {
    match l {
        Locale::ZhHans => format!("{human}失败"),
        Locale::En => format!("{human} failed"),
    }
}

pub fn tool_rewrite_title_done(l: Locale, human: &str) -> String {
    match l {
        Locale::ZhHans => format!("{human} 已完成"),
        Locale::En => format!("{human} completed"),
    }
}

pub fn tool_rewrite_title_failed_run(l: Locale, human: &str) -> String {
    match l {
        Locale::ZhHans => format!("{human} 执行失败"),
        Locale::En => format!("{human} failed"),
    }
}

pub fn tool_read_dir_label_dir(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "目录：",
        Locale::En => "dir:",
    }
}

pub fn tool_read_dir_label_shown(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展示：",
        Locale::En => "shown:",
    }
}

pub fn tool_read_dir_compact_dir_word(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "目录",
        Locale::En => "dir",
    }
}

pub fn tool_read_dir_compact_entries(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 项"),
        Locale::En => format!("{n} entries"),
    }
}

pub fn tool_read_file_label_path(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "路径：",
        Locale::En => "path:",
    }
}

pub fn tool_read_file_label_lines(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "行数：",
        Locale::En => "lines:",
    }
}

pub fn tool_read_file_default_path(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "文件",
        Locale::En => "file",
    }
}

pub fn tool_read_file_lines_suffix(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 行"),
        Locale::En => format!("{n} lines"),
    }
}

pub fn tool_search_label_keyword(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关键词：",
        Locale::En => "pattern:",
    }
}

pub fn tool_search_label_hits(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "命中：",
        Locale::En => "hits:",
    }
}

pub fn tool_search_default_keyword(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关键词",
        Locale::En => "keyword",
    }
}

pub fn tool_search_compact_keyword_word(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关键词",
        Locale::En => "keyword",
    }
}

pub fn tool_search_compact_hits_suffix(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("命中 {n} 处"),
        Locale::En => format!("{n} hits"),
    }
}

pub fn summary_line_looks_like_compact_signal(summary: &str, loc: Locale) -> bool {
    summary.lines().any(|line| {
        let zh = line.contains("输出：") || line.contains("错误输出：") || line.contains("目录：");
        let en = line.contains("Output：") || line.contains("Stderr：") || line.contains("dir ");
        match loc {
            Locale::ZhHans => zh,
            Locale::En => en || zh,
        }
    })
}

pub fn tool_failure_suggest_timeout(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "建议：缩小命令范围后重试，或提高超时阈值。",
        Locale::En => "Suggestion: retry with narrower scope or increase timeout.",
    }
}

pub fn tool_failure_suggest_invalid_args(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "建议：检查命令参数格式与路径是否正确。",
        Locale::En => "Suggestion: verify command args format and paths.",
    }
}

pub fn tool_failure_suggest_generic(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "建议：检查错误输出并按需重试。",
        Locale::En => "Suggestion: inspect stderr and retry if needed.",
    }
}

pub fn tool_failure_returned_code(l: Locale, code: &str) -> String {
    match l {
        Locale::ZhHans => format!("工具返回错误码：{code}"),
        Locale::En => format!("Tool returned error code: {code}"),
    }
}

pub fn tool_failure_no_detail(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具执行失败。",
        Locale::En => "Tool execution failed.",
    }
}

pub fn tool_failure_impact_exit(l: Locale, ec: i64) -> String {
    match l {
        Locale::ZhHans => format!("当前步骤中断（退出码：{ec}），本轮后续动作可能被跳过。"),
        Locale::En => {
            format!("This step stopped (exit code: {ec}); follow-up actions may be skipped.")
        }
    }
}

pub fn tool_failure_impact_no_exit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前步骤中断，本轮后续动作可能被跳过。",
        Locale::En => "This step stopped; follow-up actions may be skipped.",
    }
}

pub fn tool_failure_suggest_fallback(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "检查错误输出并按需重试。",
        Locale::En => "Inspect stderr and retry if needed.",
    }
}

pub fn tool_success_done_line(l: Locale, human: &str) -> String {
    match l {
        Locale::ZhHans => format!("{human} 已成功完成。"),
        Locale::En => format!("{human} completed successfully."),
    }
}

pub fn tool_success_no_output_summary(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "未返回可展示的输出摘要。",
        Locale::En => "No displayable output summary returned.",
    }
}

pub fn tool_success_next_run_command(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可继续：检查输出后执行下一条命令或进入验证。",
        Locale::En => "Next: inspect output, then run next command or verify.",
    }
}

pub fn tool_success_next_read_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可继续：基于读取结果定位修改点或继续检索相关文件。",
        Locale::En => "Next: locate edit points or continue searching related files.",
    }
}

pub fn tool_success_next_generic(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可继续：基于当前结果继续下一步操作。",
        Locale::En => "Next: continue with the next step based on this result.",
    }
}

pub fn tool_card_compact_skip_headings(l: Locale) -> [&'static str; 3] {
    match l {
        Locale::ZhHans => ["完成了什么", "产出是什么", "可继续做什么"],
        Locale::En => ["What was done", "What was produced", "What next"],
    }
}
