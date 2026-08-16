//! `expect_json_path_equals` 的路径解析：**RFC 6901 JSON Pointer**（以 `/` 开头）与 **`$` 点分遗留语法**（与历史配置兼容）。
//!
//! 非法数组下标、未闭合括号或括号后残留文本会返回 [`JsonPathResolveError::PathSyntax`]，而非静默走错路径。

use serde_json::Value;

/// JSON 路径解析失败原因（供验收失败文案区分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathResolveError {
    /// `tool_output` 不是合法 JSON。
    JsonParse(String),
    /// 路径表达式本身非法（未闭合 `[`、非数字下标、括号后多余文本等）。
    PathSyntax(String),
    /// JSON 合法且路径语法合法，但导航不到（缺字段 / 越界）。
    PathNotFound(String),
}

impl JsonPathResolveError {
    pub fn user_reason(&self) -> String {
        match self {
            Self::JsonParse(e) => format!("invalid_json: {}", e),
            Self::PathSyntax(msg) => format!("invalid_path_syntax: {}", msg),
            Self::PathNotFound(msg) => format!("path_not_found: {}", msg),
        }
    }
}

/// 提取路径指向的值；成功时克隆该子树（与历史行为一致）。
pub fn resolve_json_path_value(json_str: &str, path: &str) -> Result<Value, JsonPathResolveError> {
    let path = path.trim();
    let root: Value = serde_json::from_str(json_str.trim())
        .map_err(|e| JsonPathResolveError::JsonParse(e.to_string()))?;

    // 与历史实现一致：空白路径视为指向整份 JSON。
    if path.is_empty() {
        return Ok(root.clone());
    }

    let resolved_ref = if path.starts_with('/') {
        resolve_json_pointer_ref(&root, path)?
    } else {
        resolve_legacy_dot_path_ref(&root, path)?
    };

    Ok(resolved_ref.clone())
}

fn resolve_json_pointer_ref<'a>(
    root: &'a Value,
    pointer: &str,
) -> Result<&'a Value, JsonPathResolveError> {
    let pointer = pointer.trim();
    if !pointer.starts_with('/') {
        return Err(JsonPathResolveError::PathSyntax(
            "internal: JSON Pointer must start with '/'".into(),
        ));
    }

    let mut cur = root;
    for raw_token in pointer.split('/').skip(1) {
        let token = decode_json_pointer_token(raw_token)?;
        cur = navigate_pointer_token(cur, &token)?;
    }
    Ok(cur)
}

fn decode_json_pointer_token(raw: &str) -> Result<String, JsonPathResolveError> {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c != '~' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('0') => out.push('~'),
            Some('1') => out.push('/'),
            Some(other) => {
                return Err(JsonPathResolveError::PathSyntax(format!(
                    "invalid JSON Pointer escape ~{}",
                    other
                )));
            }
            None => {
                return Err(JsonPathResolveError::PathSyntax(
                    "truncated JSON Pointer escape".into(),
                ));
            }
        }
    }
    Ok(out)
}

fn navigate_pointer_token<'a>(
    current: &'a Value,
    token: &str,
) -> Result<&'a Value, JsonPathResolveError> {
    if let Some(arr) = current.as_array()
        && let Ok(i) = token.parse::<usize>()
    {
        return arr.get(i).ok_or_else(|| {
            JsonPathResolveError::PathNotFound(format!(
                "array index {} out of bounds (len {})",
                i,
                arr.len()
            ))
        });
    }
    current
        .get(token)
        .ok_or_else(|| JsonPathResolveError::PathNotFound(format!("missing key {:?}", token)))
}

fn resolve_legacy_dot_path_ref<'a>(
    root: &'a Value,
    path: &str,
) -> Result<&'a Value, JsonPathResolveError> {
    let segments: Vec<&str> = path
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if segments.is_empty() {
        return Err(JsonPathResolveError::PathSyntax(
            "legacy path has no segments".into(),
        ));
    }

    let mut cur = root;
    for (i, seg) in segments.iter().enumerate() {
        cur = apply_legacy_segment(cur, seg, i == 0)?;
    }
    Ok(cur)
}

fn apply_legacy_segment<'a>(
    cur: &'a Value,
    seg: &str,
    is_first: bool,
) -> Result<&'a Value, JsonPathResolveError> {
    let mut s = seg;
    if is_first && s.starts_with('$') {
        s = &s[1..];
    }

    let (field, bracket_tail) = match s.find('[') {
        Some(pos) => (s[..pos].trim(), &s[pos..]),
        None => (s.trim(), ""),
    };

    let mut v = cur;
    if !field.is_empty() {
        v = v.get(field).ok_or_else(|| {
            JsonPathResolveError::PathNotFound(format!("missing key {:?}", field))
        })?;
    }

    let indices = parse_bracket_suffix(bracket_tail)?;
    for idx in indices {
        v = v.get(idx).ok_or_else(|| {
            JsonPathResolveError::PathNotFound(format!("array index {} out of bounds", idx))
        })?;
    }
    Ok(v)
}

/// 解析紧跟在字段名后的 `[0][1]…`；必须**恰好消费**整段 `bracket_tail`，否则语法错误。
fn parse_bracket_suffix(bracket_tail: &str) -> Result<Vec<usize>, JsonPathResolveError> {
    if bracket_tail.is_empty() {
        return Ok(Vec::new());
    }

    let mut indices = Vec::new();
    let mut rest = bracket_tail;

    while !rest.is_empty() {
        let inner_rest = rest.strip_prefix('[').ok_or_else(|| {
            JsonPathResolveError::PathSyntax(format!(
                "expected '[' at start of bracket suffix, got {:?}",
                rest.chars().take(8).collect::<String>()
            ))
        })?;
        let close = inner_rest
            .find(']')
            .ok_or_else(|| JsonPathResolveError::PathSyntax("unclosed '[' in path".into()))?;
        let num = inner_rest[..close].trim();
        if num.is_empty() {
            return Err(JsonPathResolveError::PathSyntax(
                "empty array index in []".into(),
            ));
        }
        let idx = num.parse::<usize>().map_err(|_| {
            JsonPathResolveError::PathSyntax(format!(
                "invalid array index {:?} (non-negative integer required)",
                num
            ))
        })?;
        indices.push(idx);
        rest = &inner_rest[close + 1..];
    }

    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_nested_and_multi_bracket_segment() {
        let v = json!({"data":{"items":[[1,2],[3,4]]}});
        let s = v.to_string();
        let r = resolve_json_path_value(&s, "$.data.items[0][1]").unwrap();
        assert_eq!(r, json!(2));
    }

    #[test]
    fn legacy_invalid_index_is_syntax_error() {
        let v = json!({"items":[1]});
        let s = v.to_string();
        let e = resolve_json_path_value(&s, "$.items[abc]").unwrap_err();
        assert!(matches!(e, JsonPathResolveError::PathSyntax(_)));
    }

    #[test]
    fn legacy_trailing_junk_after_bracket() {
        let v = json!({"items":[1]});
        let s = v.to_string();
        let e = resolve_json_path_value(&s, "$.items[0]oops").unwrap_err();
        assert!(matches!(e, JsonPathResolveError::PathSyntax(_)));
    }

    #[test]
    fn pointer_empty_string_key_and_slash_escape() {
        let v = json!({"": 1, "a/b": 2});
        let s = v.to_string();
        // RFC 6901: "/" → 键名为空字符串。
        assert_eq!(resolve_json_path_value(&s, "/").unwrap(), json!(1));
        assert_eq!(resolve_json_path_value(&s, "/a~1b").unwrap(), json!(2));
    }

    #[test]
    fn empty_path_means_whole_document() {
        let v = json!({"x": 1});
        let s = v.to_string();
        assert_eq!(resolve_json_path_value(&s, "").unwrap(), v);
    }

    #[test]
    fn pointer_array_index() {
        let v = json!(["x", "y"]);
        let s = v.to_string();
        assert_eq!(resolve_json_path_value(&s, "/1").unwrap(), json!("y"));
    }

    #[test]
    fn json_parse_error_variant() {
        let e = resolve_json_path_value("not json", "$.a").unwrap_err();
        assert!(matches!(e, JsonPathResolveError::JsonParse(_)));
    }
}
