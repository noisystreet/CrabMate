use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// 展示用 DSML 剥离：独立 crate 内为轻量占位（完整实现仍在根包 `dsml::strip`；Web 气泡经 `message_display` 走根包 `text_sanitize`）。
#[inline]
fn strip_deepseek_dsml_for_display(s: &str) -> String {
    s.to_string()
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

static RE_ORDERED_LINE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*\d+[.)]\s+").expect("ordered list line prefix regex (static) must compile")
});

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
        return lines.into_iter().next().unwrap_or_default();
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
pub fn dedupe_plain_assistant_preamble(s: &str) -> String {
    let t = dedupe_adjacent_non_empty_trimmed_lines(s.trim());
    collapse_duplicate_prose_fused_twice(&t)
}
