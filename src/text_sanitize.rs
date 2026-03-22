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

/// 多行列表并成一句中文（分号分隔）；非全列表则合并为空格分隔的单段。
fn flatten_bullet_lines_to_prose(s: &str) -> String {
    let lines: Vec<String> = s
        .lines()
        .map(|l| l.trim().to_string())
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

/// 规划轮 assistant 去掉主 `agent_reply_plan` JSON 围栏后的**剩余正文**：不删任意 ``` 块（避免误伤合法示例），只做 DSML 清洗与列表并句。
pub fn naturalize_assistant_plan_prose_tail(s: &str) -> String {
    let t = strip_deepseek_dsml_for_display(s);
    flatten_bullet_lines_to_prose(t.trim())
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
}
