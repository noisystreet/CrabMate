//! 聊天气泡内 Markdown → 安全 HTML（`ammonia` 白名单），供助手消息渲染。
//!
//! 解析前会做 [`normalize_markdown_for_render`]，降低模型常见围栏误写导致的整段被吃进代码块等问题。

use pulldown_cmark::{Event, Options, Parser, html};

/// 在 `pulldown_cmark` 解析前做轻量规范化（单行规则，不解析嵌套结构）。
///
/// 处理常见误写：
/// 1. **行内围栏**：正文后紧贴 `` ```lang ``（如 `依赖：```rust`），拆成上一行 + 独立围栏行。
/// 2. **信息串与注释粘连**：行首合法围栏后写成 `` ```rust// comment ``，拆成 `` ```rust `` 与 `// comment` 两行。
/// 3. **行尾悬空围栏**：行首不是合法围栏行，但行尾仅剩一段 `` ``` `` 且无其它正文，去掉尾部 fence，避免误开空代码块。
/// 4. **ATX 标题缺空格**：如 `###规范与安全`（`#` 与标题字之间无空格），在至多 6 个 `#` 后补一个空格，满足 CommonMark 标题语法。
/// 5. **GFM 表头与分隔行粘连**：如 `| a | b ||---|---|`（模型常漏换行），拆成表头行 + 独立对齐行。
///
/// 无法覆盖所有非法 Markdown；极端正文若以 `` ``` `` 结尾仍可能被改写（极少见）。
pub fn normalize_markdown_for_render(md: &str) -> String {
    if md.is_empty() {
        return String::new();
    }
    md.split('\n')
        .map(normalize_one_input_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 去掉 `\r`，对单行应用围栏规范化（可能输出多行，以 `\n` 连接），再对**每一输出行**补 ATX 标题空格。
fn normalize_one_input_line(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let n = normalize_line_recursive(line);
    n.lines()
        .map(fix_atx_heading_missing_space)
        .collect::<Vec<_>>()
        .join("\n")
}

/// CommonMark ATX 标题：`#`…`#`（1–6 个）后须有空格或行尾；模型常写成 `###标题`。
fn fix_atx_heading_missing_space(line: &str) -> String {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut idx = 0usize;
    let mut indent = 0usize;
    while idx < chars.len() && indent < 3 && chars[idx].1 == ' ' {
        indent += 1;
        idx += 1;
    }
    if idx >= chars.len() {
        return line.to_string();
    }
    let hash_start = idx;
    let mut hash_end = idx;
    while hash_end < chars.len() && chars[hash_end].1 == '#' {
        hash_end += 1;
    }
    let n = hash_end - hash_start;
    if n == 0 || n > 6 {
        return line.to_string();
    }
    if hash_end >= chars.len() {
        return line.to_string();
    }
    match chars[hash_end].1 {
        ' ' | '\t' | '#' => return line.to_string(),
        _ => {}
    }
    let split_byte = chars[hash_end].0;
    let mut out = String::with_capacity(line.len() + 1);
    out.push_str(&line[..split_byte]);
    out.push(' ');
    out.push_str(&line[split_byte..]);
    out
}

fn normalize_line_recursive(line: &str) -> String {
    if let Some((header, delim)) = split_merged_table_header_separator_line(line) {
        let h = normalize_line_recursive(&header);
        let d = normalize_line_recursive(&delim);
        return format!("{h}\n{d}");
    }

    if let Some((left, right)) = split_mid_line_fence_if_needed(line) {
        let left = left.trim_end();
        let right_trim = right.trim_start_matches(' ');
        if is_fence_only_line(right) {
            return normalize_line_recursive(left);
        }
        let right_norm = normalize_line_recursive(right_trim);
        if left.is_empty() {
            return right_norm;
        }
        if right_norm.is_empty() {
            return left.to_string();
        }
        return format!("{left}\n{right_norm}");
    }

    let after_sticky = split_sticky_fence_lang_comment(line);
    normalize_trailing_orphan_fence(&after_sticky)
}

/// `| 列1 | 列2 ||------|------|` → 表头 + 对齐行（`pulldown_cmark` 要求分隔行独占一行）。
fn split_merged_table_header_separator_line(line: &str) -> Option<(String, String)> {
    if fence_starts_line(line) {
        return None;
    }
    let sp = leading_space_width(line).min(3);
    let body = line.get(sp..)?;
    if !body.trim_start().starts_with('|') {
        return None;
    }
    let indent = line.get(..sp).unwrap_or("");
    let b = body.as_bytes();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b'|' && b[i + 1] == b'|' {
            let header_body = body.get(..i)?;
            let rest = body.get(i + 2..)?;
            if table_row_looks_like_header(header_body) && looks_like_table_delimiter_row(rest) {
                let header = format!("{indent}{}", header_body.trim_end());
                let delim_trim = rest.trim_start();
                let delim_body = if delim_trim.starts_with('|') {
                    delim_trim.to_string()
                } else {
                    format!("|{delim_trim}")
                };
                let delim = format!("{indent}{delim_body}");
                return Some((header, delim));
            }
        }
        i += 1;
    }
    None
}

fn table_row_looks_like_header(row: &str) -> bool {
    let t = row.trim();
    if t.is_empty() || !t.starts_with('|') {
        return false;
    }
    if t.matches('|').count() < 2 {
        return false;
    }
    !looks_like_table_delimiter_row(t)
}

/// GFM 分隔行：由 `|` 分开的单元格，每格仅 `-` / `:` / 空白，且至少含三个 `-`。
fn looks_like_table_delimiter_row(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || !s.contains('-') {
        return false;
    }
    let parts: Vec<&str> = s
        .split('|')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().copied().all(is_delimiter_cell)
}

fn is_delimiter_cell(cell: &str) -> bool {
    if cell.is_empty() {
        return false;
    }
    let dash_count = cell.chars().filter(|&c| c == '-').count();
    if dash_count < 3 {
        return false;
    }
    cell.chars()
        .all(|c| c == '-' || c == ':' || c.is_whitespace())
}

/// 行内第一个连续 `` ` `` 段：`(字节起点, 字节长度)`，长度 ≥3。
fn first_backtick_run(line: &str) -> Option<(usize, usize)> {
    let b = line.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'`' {
            let start = i;
            let mut j = i;
            while j < b.len() && b[j] == b'`' {
                j += 1;
            }
            if j - start >= 3 {
                return Some((start, j - start));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

fn leading_space_width(line: &str) -> usize {
    line.chars().take_while(|&c| c == ' ').count()
}

/// CommonMark：围栏行前可有至多 3 个空格；其后第一个字符为 `` ` ``。
fn fence_starts_line(line: &str) -> bool {
    let sp = leading_space_width(line).min(3);
    match first_backtick_run(line) {
        Some((ix, _)) => ix == sp,
        None => false,
    }
}

/// 若首个 `` ``` `` 不在合法行首位置，拆成 `(prefix, 从 ``` 起的后缀)`。
fn split_mid_line_fence_if_needed(line: &str) -> Option<(&str, &str)> {
    let sp = leading_space_width(line).min(3);
    let (ix, _) = first_backtick_run(line)?;
    if ix == sp {
        return None;
    }
    Some((&line[..ix], &line[ix..]))
}

/// 后缀在去掉行首空格后仅为 ≥3 个 `` ` ``（可选尾随空白）。
fn is_fence_only_line(line: &str) -> bool {
    let s = line.trim_start_matches(' ');
    let mut n = 0usize;
    for ch in s.chars() {
        if ch == '`' {
            n += 1;
        } else {
            return false;
        }
    }
    n >= 3
}

/// 行首合法围栏且 info 与 `//` 粘在同一行时拆开（如 `` ```rust// x ``）。
fn split_sticky_fence_lang_comment(line: &str) -> String {
    if !fence_starts_line(line) {
        return line.to_string();
    }
    let sp = leading_space_width(line).min(3);
    let Some((ix, run_len)) = first_backtick_run(line) else {
        return line.to_string();
    };
    if ix != sp {
        return line.to_string();
    }
    let after_ticks = &line[ix + run_len..];
    // 闭合围栏：info 为空且可能只有空白
    if after_ticks.trim().is_empty() {
        return line.to_string();
    }
    let slash = after_ticks.find("//");
    let Some(slash) = slash else {
        return line.to_string();
    };
    let before_slash = &after_ticks[..slash];
    // info 段：语言 id，不含空白；允许空 info 后紧跟 `//`
    if before_slash.contains(|c: char| c.is_whitespace()) {
        return line.to_string();
    }
    if !before_slash
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+' || c == '.')
    {
        return line.to_string();
    }
    let fence_head = &line[..ix + run_len + before_slash.len()];
    let tail = &line[ix + run_len + slash..];
    format!("{fence_head}\n{tail}")
}

/// 非围栏行且行尾为「若干空格 + 纯 `` ``` ``」时去掉尾部（避免行尾误写导致下一行被吃进代码块）。
fn normalize_trailing_orphan_fence(line: &str) -> String {
    if fence_starts_line(line) {
        return line.to_string();
    }
    let trimmed_end = line.trim_end_matches([' ', '\t']);
    let Some((body, _tail)) = split_trailing_fence_run(trimmed_end) else {
        return line.to_string();
    };
    if body.is_empty() {
        return line.to_string();
    }
    // 保留原行尾换行外的尾随空白策略：用 trim 后的 body + 原行尾在 trimmed_end 之后的空白
    let suffix_ws = &line[trimmed_end.len()..];
    format!("{body}{suffix_ws}")
}

/// `text + (spaces) + ```+` → `(text, ```+)`；`text` 不得以空白结尾（避免误剥合法内联）。
fn split_trailing_fence_run(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1] == b'`' {
        i -= 1;
    }
    let tick_start = i;
    if bytes.len() - tick_start < 3 {
        return None;
    }
    let mut j = tick_start;
    while j > 0 && (bytes[j - 1] == b' ' || bytes[j - 1] == b'\t') {
        j -= 1;
    }
    if j == 0 {
        return None;
    }
    Some((&s[..j], &s[j..]))
}

/// 将 Markdown 转为经净化的 HTML 片段（不含外层 `<html>`）。
/// 段落内单换行按硬换行输出 `<br />`，避免在 `white-space: normal` 下被收成空格。
pub fn to_safe_html(md: &str) -> String {
    if md.trim().is_empty() {
        return String::new();
    }
    let md = normalize_markdown_for_render(md);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(&md, opts).map(|e| match e {
        Event::SoftBreak => Event::HardBreak,
        e => e,
    });
    let mut body = String::new();
    html::push_html(&mut body, parser);
    ammonia::clean(&body)
}

/// 调试：不做 Markdown 解析，将纯文本转义为可安全写入 `innerHTML` 的片段（换行 → `<br />`）。
pub fn plaintext_to_safe_html(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br />"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_markdown_for_render, plaintext_to_safe_html, to_safe_html};

    #[test]
    fn multi_level_headings_produce_h_tags() {
        let h = to_safe_html("# Title\n\n## Sub\n\n### H3");
        assert!(h.contains("<h1"));
        assert!(h.contains("<h2"));
        assert!(h.contains("<h3"));
    }

    #[test]
    fn empty_or_whitespace_yields_empty() {
        assert!(to_safe_html("").is_empty());
        assert!(to_safe_html("   \n\t  ").is_empty());
    }

    #[test]
    fn table_parsed_and_kept() {
        let h = to_safe_html("|h1|h2|\n|---|---|\n|a|b|");
        assert!(h.contains("<table"));
        assert!(h.to_lowercase().contains("h1"));
    }

    /// 模型/导出常把表头与 GFM 分隔行写在同一行：`||` 后应为独立一行。
    #[test]
    fn normalize_splits_table_header_glued_to_delimiter_row() {
        let raw = "| 类别 | 能力 ||------|------|";
        let n = normalize_markdown_for_render(raw);
        assert!(
            n.contains("能力") && n.contains('\n') && n.contains("------"),
            "expected two lines, got {n:?}"
        );
        let h = to_safe_html(raw);
        assert!(
            h.contains("<table"),
            "glued header+delimiter should become a table, got {h:?}"
        );
    }

    #[test]
    fn script_tag_stripped_by_ammonia() {
        let h = to_safe_html("Hello<script>alert(1)</script>");
        assert!(!h.to_lowercase().contains("<script"));
        assert!(h.contains("Hello"));
    }

    #[test]
    fn fenced_code_emits_pre_or_code() {
        let h = to_safe_html("```rust\nlet x = 1;\n```");
        assert!(h.contains("<pre") || h.contains("<code"));
    }

    #[test]
    fn normalize_splits_inline_fence_opener() {
        let raw = "`llm` 依赖：```rust\ncode()\n```\n\n|a|b|\n|---|---|\n|1|2|";
        let n = normalize_markdown_for_render(raw);
        assert!(
            n.contains("依赖：\n```rust"),
            "expected fence on its own line, got {n:?}"
        );
        let h = to_safe_html(raw);
        assert!(
            h.contains("<table"),
            "table after code fence should parse, got {h:?}"
        );
    }

    #[test]
    fn normalize_sticky_lang_slash_slash() {
        let line = "```rust// comment here";
        let n = normalize_markdown_for_render(line);
        assert_eq!(n, "```rust\n// comment here");
    }

    #[test]
    fn normalize_strips_trailing_fence_on_heading_like_line() {
        let line = "#### 仍然存在的双向依赖```";
        let n = normalize_markdown_for_render(line);
        assert_eq!(n, "#### 仍然存在的双向依赖");
    }

    #[test]
    fn normalize_preserves_valid_fence_line() {
        let line = "```rust\nlet x = 1;\n```";
        assert_eq!(normalize_markdown_for_render(line), line);
    }

    #[test]
    fn normalize_inserts_space_after_atx_hashes() {
        let raw = "###规范与安全\n\n正文。";
        assert_eq!(
            normalize_markdown_for_render(raw),
            "### 规范与安全\n\n正文。"
        );
        let h = to_safe_html(raw);
        assert!(
            h.contains("<h3") && h.contains("规范与安全"),
            "expected h3 heading, got {h:?}"
        );
    }

    #[test]
    fn single_newline_in_paragraph_emits_line_break() {
        let h = to_safe_html("不调用任何工具\n用 JSON 回复");
        let lower = h.to_lowercase();
        assert!(
            lower.contains("<br") || lower.contains("br>"),
            "expected hard line break in HTML, got {h:?}"
        );
    }

    #[test]
    fn plaintext_escapes_and_line_breaks() {
        let h = plaintext_to_safe_html("a <b>\nc");
        assert!(h.contains("&lt;"));
        assert!(h.to_lowercase().contains("<br"));
        assert!(!h.contains("<b>"));
    }
}

/// WASM 下由 `wasm-bindgen-test` 跑通「Markdown → 净化 HTML」链路（与 CSR 目标一致）。
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_bindgen_tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::to_safe_html;

    #[wasm_bindgen_test]
    fn wasm_markdown_bold_and_sanitized() {
        let h = to_safe_html("**x**");
        assert!(
            h.contains("<strong>") || h.contains("<b>"),
            "expected bold tag, got {h:?}"
        );
    }

    #[wasm_bindgen_test]
    fn wasm_markdown_table() {
        let h = to_safe_html("|c|\n|-|\n|v|");
        assert!(h.contains("<table"), "expected table, got {h:?}");
    }
}
