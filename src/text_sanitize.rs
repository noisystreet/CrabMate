//! 面向**用户可见**正文的轻量清洗（聊天、规划摘要等）。
//!
//! 与 **`redact`** 分工不同：本模块不负责日志脱敏或 HTTP 体截断。

use log::debug;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::types::{FunctionCall, Message, ToolCall};

/// 流式聚合等场景下 API 可能留下 **`function.name` 全空** 的占位 `tool_calls`，与「无 tool_calls」等价，不应阻止从正文 DSML 物化。
fn has_usable_native_tool_calls(tcs: &[ToolCall]) -> bool {
    tcs.iter().any(|tc| !tc.function.name.trim().is_empty())
}

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

/// 全角 `｜` 与 ASCII `|` 混用的 DSML 统一成 ASCII 尖括号形式，便于正则解析。
fn normalize_deepseek_dsml_brackets(s: &str) -> String {
    s.replace("<｜DSML｜", "<|DSML|")
        .replace("</｜DSML｜", "</|DSML|")
}

/// 模型常在 `<`、`|`、`DSML` 之间插空格（如 `< | DSML | invoke`），会导致整段正则匹配失败。
fn normalize_deepseek_dsml_tag_spacing(s: &str) -> String {
    static LOOSE_OPEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<\s*\|\s*DSML\s*\|").unwrap());
    static LOOSE_SHUT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"</\s*\|\s*DSML\s*\|").unwrap());
    static COMPRESS_AFTER_OPEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<\|DSML\|\s+").unwrap());
    static COMPRESS_AFTER_SHUT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"</\|DSML\|\s+").unwrap());
    let t = LOOSE_OPEN.replace_all(s, "<|DSML|");
    // 仅合并 `</` 与 `|DSML|` 之间的空白，**不要**在替换串末尾加 `>`，否则会把 `</|DSML|invoke>` 破坏成 `</|DSML|>invoke>`。
    let t = LOOSE_SHUT.replace_all(&t, "</|DSML|");
    let t = COMPRESS_AFTER_OPEN.replace_all(&t, "<|DSML|");
    COMPRESS_AFTER_SHUT.replace_all(&t, "</|DSML|>").to_string()
}

/// 仅从开标签（到第一个 `>` 为止）解析 `name="…"` / `name='…'`。
static DSML_NAME_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)name\s*=\s*["']([^"']+)["']"#).unwrap());

fn extract_dsml_name_from_open_tag(open_through_gt: &str) -> Option<String> {
    DSML_NAME_ATTR
        .captures(open_through_gt)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

const DSML_INVOKE_OPEN: &str = "<|DSML|invoke";
const DSML_INVOKE_SHUT: &str = "</|DSML|invoke>";
const DSML_PARAM_OPEN: &str = "<|DSML|parameter";
const DSML_PARAM_SHUT: &str = "</|DSML|parameter>";

/// 扫描 `invoke` 内多个 `parameter` 块（支持多行正文，不依赖跨块正则）。
fn parse_dsml_parameter_blocks(invoke_inner: &str) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    let mut i = 0usize;
    while let Some(rel) = invoke_inner[i..].find(DSML_PARAM_OPEN) {
        let block_start = i + rel;
        let after_keyword = block_start + DSML_PARAM_OPEN.len();
        let Some(gt_rel) = invoke_inner[after_keyword..].find('>') else {
            break;
        };
        let value_start = after_keyword + gt_rel + 1;
        let open_tag = &invoke_inner[block_start..value_start];
        let Some(param_name) = extract_dsml_name_from_open_tag(open_tag) else {
            i = value_start;
            continue;
        };
        let Some(shut_rel) = invoke_inner[value_start..].find(DSML_PARAM_SHUT) else {
            break;
        };
        let raw_val = invoke_inner[value_start..value_start + shut_rel].trim();
        map.insert(param_name, dsml_parameter_value_to_json(raw_val));
        i = value_start + shut_rel + DSML_PARAM_SHUT.len();
    }
    map
}

/// 返回 `(tool_name, arguments_json)`，顺序与正文一致。
fn extract_dsml_invokes(norm: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = norm[i..].find(DSML_INVOKE_OPEN) {
        let block_start = i + rel;
        let after_kw = block_start + DSML_INVOKE_OPEN.len();
        let Some(gt_rel) = norm[after_kw..].find('>') else {
            break;
        };
        let inner_start = after_kw + gt_rel + 1;
        let open_tag = &norm[block_start..inner_start];
        let Some(tool_name) = extract_dsml_name_from_open_tag(open_tag) else {
            i = inner_start;
            continue;
        };
        let Some(shut_rel) = norm[inner_start..].find(DSML_INVOKE_SHUT) else {
            break;
        };
        let inner = &norm[inner_start..inner_start + shut_rel];
        let pmap = parse_dsml_parameter_blocks(inner);
        let args = if pmap.is_empty() {
            "{}".to_string()
        } else {
            Value::Object(pmap).to_string()
        };
        out.push((tool_name, args));
        i = inner_start + shut_rel + DSML_INVOKE_SHUT.len();
    }
    out
}

/// 模型在 DSML `parameter` 里常写入 JSON 字面量（如 `["a.md"]`）；若一律当纯字符串，`run_command` 等会收到错误类型。
fn dsml_parameter_value_to_json(raw_val: &str) -> Value {
    let trimmed = raw_val.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(trimmed.to_string()))
}

/// 部分 DeepSeek 兼容端在 **`tool_calls` 为空** 时，仍把调用写在正文 **DSML**（`<|DSML|invoke>`）里。
/// TUI 展示会 [`strip_deepseek_dsml_for_display`] 剥掉这些标记，用户易误以为「只有说明、没调工具」。
/// 在 Agent 侧若 API 未给 `tool_calls`，则从 **`content` 与 `reasoning_content` 拼接文本**解析并写入 `msg.tool_calls`，并分别剥除两字段中的 DSML 以节省后续 token。
pub fn materialize_deepseek_dsml_tool_calls_in_message(msg: &mut Message) {
    if msg
        .tool_calls
        .as_ref()
        .is_some_and(|c| !c.is_empty() && has_usable_native_tool_calls(c))
    {
        return;
    }
    let c = msg.content.as_deref().unwrap_or("");
    let r = msg.reasoning_content.as_deref().unwrap_or("");
    if c.is_empty() && r.is_empty() {
        return;
    }
    let looks_dsml = [c, r]
        .iter()
        .any(|s| s.contains("DSML") || s.contains("dsml") || s.contains(DSML_OPEN_FW));
    if !looks_dsml {
        return;
    }
    let combined = if c.is_empty() {
        r.to_string()
    } else if r.is_empty() {
        c.to_string()
    } else {
        format!("{c}\n{r}")
    };
    let norm = normalize_deepseek_dsml_tag_spacing(&normalize_deepseek_dsml_brackets(&combined));
    let parsed = extract_dsml_invokes(&norm);
    if parsed.is_empty() {
        return;
    }
    let mut out_calls: Vec<ToolCall> = Vec::new();
    for (i, (tool_name, arguments)) in parsed.into_iter().enumerate() {
        out_calls.push(ToolCall {
            id: format!("dsml_{i}"),
            typ: "function".to_string(),
            function: FunctionCall {
                name: tool_name,
                arguments,
            },
        });
    }
    debug!(
        target: "crabmate",
        "从助手正文 DeepSeek DSML 解析出 {} 个 tool_calls（API 未提供 tool_calls）",
        out_calls.len()
    );
    msg.tool_calls = Some(out_calls);
    fn trim_stripped_field(s: &mut Option<String>) {
        let Some(t) = s.as_deref() else {
            return;
        };
        let stripped = strip_deepseek_dsml_for_display(t);
        let u = stripped.trim();
        if u.is_empty() {
            *s = None;
        } else {
            *s = Some(u.to_string());
        }
    }
    trim_stripped_field(&mut msg.content);
    trim_stripped_field(&mut msg.reasoning_content);
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
    let mapped: String = trim_assistant_prose_line(s)
        .chars()
        .map(|c| match c {
            '\u{ff1a}' => ':',
            '\u{ff0c}' => ',',
            '\u{ff01}' => '!',
            '\u{ff1f}' => '?',
            _ => c,
        })
        .collect();
    // 仅用于「相邻同句」判等：忽略句末语气/冒号差异，避免“同句仅标点不同”去重失败。
    mapped
        .trim_end_matches(|c: char| {
            matches!(c, '。' | '！' | '？' | '：' | '…' | '.' | '!' | '?' | ':')
        })
        .trim()
        .to_string()
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
    fn naturalize_plan_prose_dedupes_terminal_punctuation_variant() {
        let a = "我将先拆解任务步骤：";
        let b = "我将先拆解任务步骤。";
        let raw = format!("{a}\n{b}");
        assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
    }

    #[test]
    fn dedupe_plain_preamble_collapses_space_joined_duplicate() {
        let line = "我将帮您编写 Hello World 并规划步骤。";
        let raw = format!("{line} {line}");
        assert_eq!(dedupe_plain_assistant_preamble(&raw), line);
    }

    #[test]
    fn materialize_dsml_populates_tool_calls_and_strips_markup() {
        let dsml = r#"将更新文件。
<|DSML|function_calls>
<|DSML|invoke name="modify_file">
<|DSML|parameter name="path">1.md</|DSML|parameter>
<|DSML|parameter name="content"># 标题</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "modify_file");
        let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
        assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
        assert_eq!(v.get("content").and_then(|x| x.as_str()), Some("# 标题"));
        let prose = msg.content.as_deref().unwrap_or("");
        assert!(prose.contains("将更新"));
        assert!(!prose.contains("DSML"));
    }

    #[test]
    fn materialize_dsml_spaced_tags_and_multiline_parameter() {
        let dsml = r#"将写入。
< | DSML | invoke name="modify_file" >
<|DSML|parameter name="path">note.md</|DSML|parameter>
<|DSML|parameter name="content"># 标题
第二行</|DSML|parameter>
</|DSML|invoke>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "modify_file");
        let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
        assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("note.md"));
        assert!(
            v.get("content")
                .and_then(|x| x.as_str())
                .is_some_and(|s| s.contains("第二行"))
        );
    }

    #[test]
    fn materialize_dsml_single_quoted_names() {
        let dsml = r#"<|DSML|invoke name='read_file'>
<|DSML|parameter name='path'>x.txt</|DSML|parameter>
</|DSML|invoke>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs[0].function.name, "read_file");
    }

    #[test]
    fn materialize_dsml_json_array_parameter_for_run_command_args() {
        let dsml = r#"让我用 cat。
<|DSML|function_calls>
<|DSML|invoke name="run_command">
<|DSML|parameter name="command" string="true">cat</|DSML|parameter>
<|DSML|parameter name="args" string="true">["1.md"]</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs[0].function.name, "run_command");
        let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
        assert_eq!(v.get("command").and_then(|x| x.as_str()), Some("cat"));
        let args = v
            .get("args")
            .and_then(|x| x.as_array())
            .expect("args array");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].as_str(), Some("1.md"));
    }

    #[test]
    fn materialize_dsml_from_reasoning_when_content_empty() {
        let dsml = r#"<|DSML|invoke name="read_file">
<|DSML|parameter name="path">z.txt</|DSML|parameter>
</|DSML|invoke>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: Some(dsml.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs[0].function.name, "read_file");
        assert!(
            msg.reasoning_content
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        );
    }

    #[test]
    fn materialize_dsml_replaces_nameless_native_tool_call_placeholders() {
        let dsml = r#"说明文字。
<|DSML|function_calls>
<|DSML|invoke name="modify_file">
<|DSML|parameter name="path" string="true">1.md</|DSML|parameter>
<|DSML|parameter name="content" string="true"># Hi

Line2</|DSML|parameter>
</|DSML|invoke>
</|DSML|function_calls>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "stream_slot_0".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: String::new(),
                    arguments: String::new(),
                },
            }]),
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "modify_file");
        let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
        assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
        assert!(
            v.get("content")
                .and_then(|x| x.as_str())
                .is_some_and(|s| s.contains("Line2"))
        );
    }

    #[test]
    fn materialize_dsml_fullwidth_brackets_create_file_like_cli() {
        // 与部分模型在规划轮输出的全角 `｜` DSML 一致（分阶段路径须先物化再执行工具）。
        let dsml = "我们只需要创建 1.md。<｜DSML｜function_calls>\n\
<｜DSML｜invoke name=\"create_file\">\n\
<｜DSML｜parameter name=\"path\" string=\"true\">1.md</｜DSML｜parameter>\n\
<｜DSML｜parameter name=\"content\" string=\"true\"></｜DSML｜parameter>\n\
</｜DSML｜invoke>\n\
</｜DSML｜function_calls>";
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "create_file");
        let v: Value = serde_json::from_str(&tcs[0].function.arguments).unwrap();
        assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("1.md"));
        assert_eq!(v.get("content").and_then(|x| x.as_str()), Some(""));
        assert!(!msg.content.as_deref().unwrap_or("").contains("DSML"));
    }

    #[test]
    fn materialize_dsml_skipped_when_native_tool_call_has_name() {
        let dsml = r#"<|DSML|invoke name="modify_file">
<|DSML|parameter name="path">x.md</|DSML|parameter>
</|DSML|invoke>"#;
        let mut msg = Message {
            role: "assistant".to_string(),
            content: Some(dsml.to_string()),
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "real".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        };
        materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
        let tcs = msg.tool_calls.as_ref().expect("tool_calls");
        assert_eq!(tcs[0].function.name, "read_file");
    }
}
