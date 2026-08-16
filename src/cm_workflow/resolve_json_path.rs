//! JSON 路径解析（从 `crabmate-agent` 的 `acceptance::resolve_json_path_value` 移植）。
//!
//! 支持 **RFC 6901 JSON Pointer**（以 `/` 开头）与 **`$` 点分遗留语法**。

use serde_json::Value;

/// JSON 路径解析失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathResolveError {
    /// `tool_output` 不是合法 JSON。
    JsonParse(String),
    /// 路径表达式本身非法。
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

/// 提取路径指向的值；成功时克隆该子树。
pub fn resolve_json_path_value(json_str: &str, path: &str) -> Result<Value, JsonPathResolveError> {
    let path = path.trim();
    let root: Value = serde_json::from_str(json_str.trim())
        .map_err(|e| JsonPathResolveError::JsonParse(e.to_string()))?;

    // 空白路径视为指向整份 JSON。
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

/// 解析紧跟在字段名后的 `[0][1]…`；必须**恰好消费**整段 `bracket_tail`。
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
