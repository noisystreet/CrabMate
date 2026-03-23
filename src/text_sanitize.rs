//! 面向**用户可见**正文的轻量清洗（聊天、规划摘要等）。
//!
//! 与 **`redact`** 分工不同：本模块不负责日志脱敏或 HTTP 体截断。

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const DSML_OPEN_FW: &str = "<｜DSML｜";
const DSML_CLOSE_FW: &str = "</｜DSML｜";
const DSML_OPEN_ASCII: &str = "<|DSML|";
const DSML_CLOSE_ASCII: &str = "</|DSML|";

/// `regex` crate 不支持反向引用，未知标签用扫描配对闭合 `</｜DSML｜tag>`。
fn strip_dsml_named_blocks_fullwidth(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(start) = out.find(DSML_OPEN_FW) else {
            break;
        };
        let rest = &out[start + DSML_OPEN_FW.len()..];
        let tag_end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len().min(64));
        let tag = rest.get(..tag_end).unwrap_or("");
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.replace_range(start..start.saturating_add(1), "");
            continue;
        }
        let close = format!("{DSML_CLOSE_FW}{tag}>");
        if let Some(rel) = out[start..].find(&close) {
            let end = start + rel + close.len();
            out.replace_range(start..end, "");
        } else if let Some(rel) = out[start..].find('>') {
            let end = start + rel + 1;
            out.replace_range(start..end, "");
        } else {
            out.replace_range(start..start.saturating_add(1), "");
        }
    }
    out
}

fn strip_dsml_named_blocks_ascii(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(start) = out.find(DSML_OPEN_ASCII) else {
            break;
        };
        let rest = &out[start + DSML_OPEN_ASCII.len()..];
        let tag_end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len().min(64));
        let tag = rest.get(..tag_end).unwrap_or("");
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.replace_range(start..start.saturating_add(1), "");
            continue;
        }
        let close = format!("{DSML_CLOSE_ASCII}{tag}>");
        if let Some(rel) = out[start..].find(&close) {
            let end = start + rel + close.len();
            out.replace_range(start..end, "");
        } else if let Some(rel) = out[start..].find('>') {
            let end = start + rel + 1;
            out.replace_range(start..end, "");
        } else {
            out.replace_range(start..start.saturating_add(1), "");
        }
    }
    out
}

fn collapse_blank_runs(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    lines
        .split(|line| line.is_empty())
        .map(|g| g.join("\n"))
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

/// 去掉 DeepSeek **DSML**（`<｜DSML｜…>` / `<|DSML|…>` 等结构化片段，常见于 `function_calls` / `invoke` / `parameter`），
/// 避免规划与对话区出现标记而非自然语言。
pub fn strip_deepseek_dsml_for_display(s: &str) -> String {
    if !s.contains("DSML") {
        return s.to_string();
    }
    let mut out = s.to_string();

    static ORDERED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        vec![
            Regex::new(r"(?s)<｜DSML｜parameter\b[^>]*>.*?</｜DSML｜parameter>").unwrap(),
            Regex::new(r"(?s)<｜DSML｜invoke\b[^>]*>.*?</｜DSML｜invoke>").unwrap(),
            Regex::new(r"(?s)<｜DSML｜function_calls\b[^>]*>.*?</｜DSML｜function_calls>").unwrap(),
            Regex::new(r"(?s)<\|DSML\|parameter\b[^>]*>.*?</\|DSML\|parameter>").unwrap(),
            Regex::new(r"(?s)<\|DSML\|invoke\b[^>]*>.*?</\|DSML\|invoke>").unwrap(),
            Regex::new(r"(?s)<\|DSML\|function_calls\b[^>]*>.*?</\|DSML\|function_calls>").unwrap(),
        ]
    });
    for re in ORDERED.iter() {
        loop {
            let next = re.replace_all(&out, "").to_string();
            if next == out {
                break;
            }
            out = next;
        }
    }

    out = strip_dsml_named_blocks_fullwidth(&out);
    out = strip_dsml_named_blocks_ascii(&out);

    // 未闭合或单行残留的开闭标签
    static ORPHAN_OPEN_FW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<｜DSML｜[^>\n]{0,300}>").unwrap());
    static ORPHAN_CLOSE_FW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"</｜DSML｜[^>\n]{1,80}>").unwrap());
    static ORPHAN_OPEN_ASCII: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<\|DSML\|[^>\n]{0,300}>").unwrap());
    static ORPHAN_CLOSE_ASCII: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"</\|DSML\|[^>\n]{1,80}>").unwrap());
    out = ORPHAN_OPEN_FW.replace_all(&out, "").to_string();
    out = ORPHAN_CLOSE_FW.replace_all(&out, "").to_string();
    out = ORPHAN_OPEN_ASCII.replace_all(&out, "").to_string();
    out = ORPHAN_CLOSE_ASCII.replace_all(&out, "").to_string();

    collapse_blank_runs(&out)
}

fn strip_markdown_fenced_blocks(s: &str) -> String {
    let parts: Vec<&str> = s.split("```").collect();
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i % 2 == 0 {
            out.push_str(part);
        }
    }
    out
}

fn step_object_to_line(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("description")
        .and_then(|x| x.as_str())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            obj.get("id")
                .and_then(|x| x.as_str())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .map(|id| format!("完成步骤「{id}」"))
        })
}

/// 若整段是 JSON 对象/单元素数组，抽出 `description` 或 `id` 作展示句。
fn try_unwrap_embedded_step_json(t: &str) -> Option<String> {
    let t = t.trim();
    if !t.starts_with('{') && !t.starts_with('[') {
        return None;
    }
    let v: Value = serde_json::from_str(t).ok()?;
    if let Some(arr) = v.as_array() {
        if arr.len() == 1 {
            return arr
                .first()
                .and_then(|x| x.as_object())
                .and_then(step_object_to_line);
        }
        return None;
    }
    if let Some(obj) = v.as_object() {
        if let Some(steps) = obj.get("steps").and_then(|x| x.as_array())
            && steps.len() == 1
        {
            return steps
                .first()
                .and_then(|x| x.as_object())
                .and_then(step_object_to_line);
        }
        return step_object_to_line(obj);
    }
    None
}

static RE_ORDERED_LINE_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d+[.)]\s+").unwrap());

/// 行首尾的空白、BOM、零宽字符（模型/API 偶发夹带）；`str::trim()` 不会去掉 U+200B。
fn trim_assistant_prose_line(s: &str) -> String {
    s.replace('\u{a0}', " ")
        .trim()
        .trim_matches(|c: char| {
            matches!(
                c,
                '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}'
            )
        })
        .to_string()
}

/// 仅用于**判等**：全角标点与 ASCII 混用时仍视为同一句，避免相邻行「看起来一样」却去重失败。
fn prose_dedup_normalize(s: &str) -> String {
    trim_assistant_prose_line(s)
        .chars()
        .map(|c| match c {
            '\u{ff1a}' => ':',
            '\u{ff0c}' => ',',
            '\u{ff01}' => '!',
            '\u{ff1f}' => '?',
            _ => c,
        })
        .collect()
}

/// 去掉**相邻**的、去首尾空白后完全相同的非空行（模型在围栏前偶发整段复读）。
fn dedupe_adjacent_non_empty_trimmed_lines(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in s.lines() {
        let t = trim_assistant_prose_line(line);
        if t.is_empty() {
            continue;
        }
        if out
            .last()
            .is_some_and(|last| prose_dedup_normalize(last) == prose_dedup_normalize(&t))
        {
            continue;
        }
        out.push(t);
    }
    out.join("\n")
}

/// `flatten_bullet_lines_to_prose` 会把多行非列表用空格并成一段；若两行本为同一句，会变成 `A A`，此处再折回单段。
/// 亦处理「半段字符级完全重复」的整串（无分隔符复制粘贴）。
fn collapse_duplicate_prose_fused_twice_once(s: &str) -> String {
    const MIN_CHARS: usize = 12;
    let t = trim_assistant_prose_line(s);
    if t.is_empty() {
        return String::new();
    }
    let n = t.chars().count();
    if n < MIN_CHARS * 2 {
        return t;
    }
    let chars: Vec<char> = t.chars().collect();
    let n = chars.len();
    // 字符级半段相等：`ABCABC`
    if n.is_multiple_of(2) {
        let h = n / 2;
        let left: String = chars[..h].iter().collect();
        let right: String = chars[h..].iter().collect();
        let l = trim_assistant_prose_line(&left);
        let r = trim_assistant_prose_line(&right);
        if l.chars().count() >= MIN_CHARS && prose_dedup_normalize(&l) == prose_dedup_normalize(&r)
        {
            return l;
        }
    }
    // 任意切分：左右 trim 后全文相同（中间可为空白）
    for i in (MIN_CHARS..=n.saturating_sub(MIN_CHARS)).rev() {
        let left: String = chars[..i].iter().collect();
        let right: String = chars[i..].iter().collect();
        let l = trim_assistant_prose_line(&left);
        let r = trim_assistant_prose_line(&right);
        if l.chars().count() >= MIN_CHARS && prose_dedup_normalize(&l) == prose_dedup_normalize(&r)
        {
            return l;
        }
    }
    t
}

fn collapse_duplicate_prose_fused_twice(s: &str) -> String {
    let mut t = s.to_string();
    loop {
        let next = collapse_duplicate_prose_fused_twice_once(&t);
        if next == t {
            return t;
        }
        t = next;
    }
}

/// 多行列表并成一句中文（分号分隔）；非全列表则合并为空格分隔的单段。
fn flatten_bullet_lines_to_prose(s: &str) -> String {
    let lines: Vec<String> = s
        .lines()
        .map(trim_assistant_prose_line)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    if lines.len() == 1 {
        return lines.into_iter().next().unwrap();
    }
    let all_bullets = lines.iter().all(|l| {
        l.starts_with("- ")
            || l.starts_with("* ")
            || l.starts_with("• ")
            || RE_ORDERED_LINE_PREFIX.is_match(l)
    });
    let cleaned: Vec<String> = lines
        .into_iter()
        .map(|l| {
            if let Some(x) = l.strip_prefix("- ") {
                x.to_string()
            } else if let Some(x) = l.strip_prefix("* ") {
                x.to_string()
            } else if let Some(x) = l.strip_prefix("• ") {
                x.to_string()
            } else {
                RE_ORDERED_LINE_PREFIX
                    .replace(l.trim(), "")
                    .trim()
                    .to_string()
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    if all_bullets && cleaned.len() > 1 {
        cleaned.join("；")
    } else {
        cleaned.join(" ")
    }
}

/// 规划里单条 `description` / `id` 的展示用：去 DSML、去掉误贴的代码围栏、展开嵌套 JSON、Markdown 列表改叙述。
pub fn naturalize_plan_step_description(s: &str) -> String {
    let mut t = strip_deepseek_dsml_for_display(s);
    t = strip_markdown_fenced_blocks(&t);
    let trimmed = t.trim();
    let mut out = if let Some(inner) = try_unwrap_embedded_step_json(trimmed) {
        inner
    } else {
        trimmed.to_string()
    };
    let t2 = out.trim();
    if (t2.starts_with('{') || t2.starts_with('['))
        && let Some(inner) = try_unwrap_embedded_step_json(t2)
    {
        out = inner;
    }
    flatten_bullet_lines_to_prose(&out)
}

/// 规划轮 assistant 去掉主 `agent_reply_plan` JSON 围栏后的**剩余正文**：不删任意 ``` 块（避免误伤合法示例），只做 DSML 清洗、相邻重复行折叠、列表并句与「并句后仍整段双份」折叠。
pub fn naturalize_assistant_plan_prose_tail(s: &str) -> String {
    let t = strip_deepseek_dsml_for_display(s);
    let t = dedupe_adjacent_non_empty_trimmed_lines(t.trim());
    let flat = flatten_bullet_lines_to_prose(t.trim());
    collapse_duplicate_prose_fused_twice(&flat)
}

/// 尚无 Markdown 围栏、且非整段 JSON 的助手正文：去掉围栏前同句复读（打出 ``` 之前的流式阶段走此路径，原先不经 [`naturalize_assistant_plan_prose_tail`]）。
pub(crate) fn dedupe_plain_assistant_preamble(s: &str) -> String {
    let t = dedupe_adjacent_non_empty_trimmed_lines(s.trim());
    collapse_duplicate_prose_fused_twice(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naturalize_step_extracts_json_description() {
        let raw = r#"{"id":"a","description":"读取配置并汇总"}"#;
        assert_eq!(naturalize_plan_step_description(raw), "读取配置并汇总");
    }

    #[test]
    fn naturalize_step_flattens_markdown_list() {
        let s = "- 查日志\n- 修配置";
        assert_eq!(naturalize_plan_step_description(s), "查日志；修配置");
    }

    #[test]
    fn strips_nested_dsml_fullwidth() {
        let s = "前言<｜DSML｜function_calls><｜DSML｜invoke name=\"f\"><｜DSML｜parameter name=\"x\" string=\"true\">v</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜function_calls>后记";
        let t = strip_deepseek_dsml_for_display(s);
        assert!(!t.contains("DSML"));
        assert!(t.contains("前言"));
        assert!(t.contains("后记"));
    }

    #[test]
    fn strips_ascii_pipe_variant() {
        let s = "a <|DSML|function_calls></|DSML|function_calls> b";
        let t = strip_deepseek_dsml_for_display(s);
        assert!(!t.contains("DSML"));
        assert!(t.contains('a'));
        assert!(t.contains('b'));
    }

    #[test]
    fn noop_without_dsml() {
        let s = "普通中文与 English\n第二行";
        assert_eq!(strip_deepseek_dsml_for_display(s), s);
    }

    #[test]
    fn naturalize_plan_prose_dedupes_adjacent_identical_lines() {
        let line = "我将帮您编写 Hello World，并先规划任务步骤：";
        let raw = format!("{line}\n{line}");
        assert_eq!(naturalize_assistant_plan_prose_tail(&raw), line);
    }

    #[test]
    fn naturalize_plan_prose_dedupes_fullwidth_colon_variant() {
        let a = "我将帮您编写步骤：";
        let b = "我将帮您编写步骤:"; // ASCII colon
        let raw = format!("{a}\n{b}");
        assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
    }

    #[test]
    fn dedupe_plain_preamble_collapses_space_joined_duplicate() {
        let line = "我将帮您编写 Hello World 并规划步骤。";
        let raw = format!("{line} {line}");
        assert_eq!(dedupe_plain_assistant_preamble(&raw), line);
    }
}
