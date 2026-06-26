//! DSML 正文解析：`invoke` / `parameter` → 工具名与 JSON 参数字符串。

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use super::normalizer::normalize_for_parse;
use super::strip_scan::DSML_OPEN_FW;

/// 单次从正文中解析出的工具调用（与 [`crabmate_types::ToolCall`] 解耦，便于单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDsmlInvoke {
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DsmlParseOutcome {
    pub invokes: Vec<ParsedDsmlInvoke>,
    pub had_dsml_markers: bool,
}

static DSML_NAME_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)name\s*=\s*["']([^"']+)["']"#)
        .expect("DSML name= attribute regex (static pattern) must compile")
});

const DSML_INVOKE_OPEN: &str = "<|DSML|invoke";
const DSML_INVOKE_SHUT: &str = "</|DSML|invoke>";
const DSML_PARAM_OPEN: &str = "<|DSML|parameter";
const DSML_PARAM_SHUT: &str = "</|DSML|parameter>";

pub fn text_looks_like_dsml(content: &str, reasoning: &str) -> bool {
    [content, reasoning]
        .iter()
        .any(|s| s.contains("DSML") || s.contains("dsml") || s.contains(DSML_OPEN_FW))
}

pub fn combine_assistant_text_fields(content: &str, reasoning: &str) -> String {
    if content.is_empty() {
        reasoning.to_string()
    } else if reasoning.is_empty() {
        content.to_string()
    } else {
        format!("{content}\n{reasoning}")
    }
}

/// 纯函数：从 `content` + `reasoning` 拼接文本解析 DSML，不写 [`crabmate_types::Message`]。
pub fn parse_combined_assistant_text(content: &str, reasoning: &str) -> DsmlParseOutcome {
    if content.is_empty() && reasoning.is_empty() {
        return DsmlParseOutcome::default();
    }
    let had_dsml_markers = text_looks_like_dsml(content, reasoning);
    if !had_dsml_markers {
        return DsmlParseOutcome {
            had_dsml_markers: false,
            ..Default::default()
        };
    }
    let combined = combine_assistant_text_fields(content, reasoning);
    let norm = normalize_for_parse(&combined);
    let invokes = extract_dsml_invokes(&norm)
        .into_iter()
        .map(|(name, arguments_json)| ParsedDsmlInvoke {
            name,
            arguments_json,
        })
        .collect();
    DsmlParseOutcome {
        invokes,
        had_dsml_markers,
    }
}

fn extract_dsml_name_from_open_tag(open_through_gt: &str) -> Option<String> {
    DSML_NAME_ATTR
        .captures(open_through_gt)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

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

fn dsml_parameter_value_to_json(raw_val: &str) -> Value {
    let trimmed = raw_val.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(trimmed.to_string()))
}
