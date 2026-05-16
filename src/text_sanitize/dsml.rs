use log::debug;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::text_sanitize_dsml_vendor::normalize_deepseek_dsml_vendor_variants;
use crate::types::{FunctionCall, Message, MessageContent, ToolCall};

#[path = "dsml_strip_scan.rs"]
mod dsml_strip_scan;

use dsml_strip_scan::{
    collapse_blank_runs, strip_dsml_named_blocks_ascii, strip_dsml_named_blocks_fullwidth,
};

/// 流式聚合等场景下 API 可能留下 **`function.name` 全空** 的占位 `tool_calls`，与「无 tool_calls」等价，不应阻止从正文 DSML 物化。
fn has_usable_native_tool_calls(tcs: &[ToolCall]) -> bool {
    tcs.iter().any(|tc| !tc.function.name.trim().is_empty())
}

// 编译期固定的 DSML 块正则；若失败说明模式字符串损坏，应在 CI 暴露。
// 放在函数外：避免嵌套 `static` 触发 lizard 等工具的 Rust 解析边界问题（误把后续函数并入上一函数体）。
static STRIP_DSML_ORDERED_BLOCK_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    const PATTERNS: &[&str] = &[
        // 外层 `tool_calls`（DeepSeek 常见；先于内层块移除）
        r"(?s)<｜DSML｜tool_calls\b[^>]*>.*?</｜DSML｜tool_calls>",
        r"(?s)<\|DSML\|tool_calls\b[^>]*>.*?</\|DSML\|tool_calls>",
        r"(?s)<｜DSML｜parameter\b[^>]*>.*?</｜DSML｜parameter>",
        r"(?s)<｜DSML｜invoke\b[^>]*>.*?</｜DSML｜invoke>",
        r"(?s)<｜DSML｜function_calls\b[^>]*>.*?</｜DSML｜function_calls>",
        r"(?s)<\|DSML\|parameter\b[^>]*>.*?</\|DSML\|parameter>",
        r"(?s)<\|DSML\|invoke\b[^>]*>.*?</\|DSML\|invoke>",
        r"(?s)<\|DSML\|function_calls\b[^>]*>.*?</\|DSML\|function_calls>",
    ];
    PATTERNS
        .iter()
        .map(|p| Regex::new(p).expect("strip_deepseek_dsml: static DSML block regex must compile"))
        .collect()
});

// 未闭合或单行残留的开闭标签
static STRIP_DSML_ORPHAN_OPEN_FW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<｜DSML｜[^>\n]{0,300}>")
        .expect("strip_deepseek_dsml: orphan fullwidth open-tag regex must compile")
});
static STRIP_DSML_ORPHAN_CLOSE_FW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</｜DSML｜[^>\n]{1,80}>")
        .expect("strip_deepseek_dsml: orphan fullwidth close-tag regex must compile")
});
static STRIP_DSML_ORPHAN_OPEN_ASCII: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<\|DSML\|[^>\n]{0,300}>")
        .expect("strip_deepseek_dsml: orphan ASCII open-tag regex must compile")
});
static STRIP_DSML_ORPHAN_CLOSE_ASCII: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</\|DSML\|[^>\n]{1,80}>")
        .expect("strip_deepseek_dsml: orphan ASCII close-tag regex must compile")
});

/// 去掉 DeepSeek **DSML**（`<｜DSML｜…>` / `<|DSML|…>` 等结构化片段，常见于 `function_calls` / `invoke` / `parameter`），
/// 避免规划与对话区出现标记而非自然语言。
pub fn strip_deepseek_dsml_for_display(s: &str) -> String {
    let s = normalize_deepseek_dsml_vendor_variants(s);
    if !s.contains("DSML") {
        return s;
    }
    let mut out = s;

    for re in STRIP_DSML_ORDERED_BLOCK_RES.iter() {
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

    out = STRIP_DSML_ORPHAN_OPEN_FW.replace_all(&out, "").to_string();
    out = STRIP_DSML_ORPHAN_CLOSE_FW.replace_all(&out, "").to_string();
    out = STRIP_DSML_ORPHAN_OPEN_ASCII
        .replace_all(&out, "")
        .to_string();
    out = STRIP_DSML_ORPHAN_CLOSE_ASCII
        .replace_all(&out, "")
        .to_string();

    collapse_blank_runs(&out)
}

/// 全角 `｜` 与 ASCII `|` 混用的 DSML 统一成 ASCII 尖括号形式，便于正则解析。
fn normalize_deepseek_dsml_brackets(s: &str) -> String {
    let s = normalize_deepseek_dsml_vendor_variants(s);
    s.replace("<｜DSML｜", "<|DSML|")
        .replace("</｜DSML｜", "</|DSML|")
}

/// 模型常在 `<`、`|`、`DSML` 之间插空格（如 `< | DSML | invoke`），会导致整段正则匹配失败。
fn normalize_deepseek_dsml_tag_spacing(s: &str) -> String {
    static LOOSE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<\s*\|\s*DSML\s*\|")
            .expect("normalize_dsml_spacing: loose open-tag regex must compile")
    });
    static LOOSE_SHUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"</\s*\|\s*DSML\s*\|")
            .expect("normalize_dsml_spacing: loose close-prefix regex must compile")
    });
    static COMPRESS_AFTER_OPEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<\|DSML\|\s+")
            .expect("normalize_dsml_spacing: compress-after-open regex must compile")
    });
    static COMPRESS_AFTER_SHUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"</\|DSML\|\s+")
            .expect("normalize_dsml_spacing: compress-after-shut regex must compile")
    });
    let t = LOOSE_OPEN.replace_all(s, "<|DSML|");
    // 仅合并 `</` 与 `|DSML|` 之间的空白，**不要**在替换串末尾加 `>`，否则会把 `</|DSML|invoke>` 破坏成 `</|DSML|>invoke>`。
    let t = LOOSE_SHUT.replace_all(&t, "</|DSML|");
    let t = COMPRESS_AFTER_OPEN.replace_all(&t, "<|DSML|");
    COMPRESS_AFTER_SHUT.replace_all(&t, "</|DSML|>").to_string()
}

/// 仅从开标签（到第一个 `>` 为止）解析 `name="…"` / `name='…'`。
static DSML_NAME_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)name\s*=\s*["']([^"']+)["']"#)
        .expect("DSML name= attribute regex (static pattern) must compile")
});

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
///
/// `enabled == false` 时本函数立即返回（与配置 **`materialize_deepseek_dsml_tool_calls`** 对齐）：仅使用 API 原生 `tool_calls`。
/// 更稳的结构化路径是网关始终返回 OpenAI 式 `tool_calls`；或约定正文**仅一段 JSON** 工具调用并由专用解析器处理（当前未实现，与 DSML 二选一由产品定）。
pub fn materialize_deepseek_dsml_tool_calls_in_message(msg: &mut Message, enabled: bool) {
    if !enabled {
        return;
    }
    if msg
        .tool_calls
        .as_ref()
        .is_some_and(|c| !c.is_empty() && has_usable_native_tool_calls(c))
    {
        return;
    }
    let c = crate::types::message_content_as_str(&msg.content).unwrap_or("");
    let r = msg.reasoning_content.as_deref().unwrap_or("");
    if c.is_empty() && r.is_empty() {
        return;
    }
    let looks_dsml = [c, r].iter().any(|s| {
        s.contains("DSML") || s.contains("dsml") || s.contains(dsml_strip_scan::DSML_OPEN_FW)
    });
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
    if let Some(MessageContent::Text(ref mut s)) = msg.content {
        let stripped = strip_deepseek_dsml_for_display(s);
        let u = stripped.trim();
        if u.is_empty() {
            msg.content = None;
        } else {
            *s = u.to_string();
        }
    }
    trim_stripped_field(&mut msg.reasoning_content);
}
