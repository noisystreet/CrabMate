//! 工具调用入参 JSON（`args_json`）的公共解析，统一错误文案。

/// 将工具入参 JSON 解析为 [`serde_json::Value`]；失败时返回与用户可见 runner 一致的中文说明。
#[inline]
pub(crate) fn parse_args_json(args_json: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {e}"))
}

/// 仅用于 `run_command`：尝试修复常见的裸 token 数组写法（如 `args:[-la]`、`args:[CMakeLists.txt]`）。
/// 返回修复后的 JSON 字符串；若不满足修复条件则返回 `None`。
pub(crate) fn try_repair_run_command_args_json(raw: &str) -> Option<String> {
    let args_key = "\"args\"";
    let key_pos = raw.find(args_key)?;
    let after_key = &raw[key_pos + args_key.len()..];
    let colon_rel = after_key.find(':')?;
    let after_colon_abs = key_pos + args_key.len() + colon_rel + 1;
    let after_colon = &raw[after_colon_abs..];
    let lb_rel = after_colon.find('[')?;
    let lb_abs = after_colon_abs + lb_rel;

    let mut depth = 0i32;
    let mut rb_abs = None;
    for (i, ch) in raw[lb_abs..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    rb_abs = Some(lb_abs + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let rb_abs = rb_abs?;
    let inner = &raw[lb_abs + 1..rb_abs];
    let repaired_inner = repair_args_array_inner(inner)?;
    let mut out = String::with_capacity(raw.len() + 16);
    out.push_str(&raw[..lb_abs + 1]);
    out.push_str(&repaired_inner);
    out.push_str(&raw[rb_abs..]);
    Some(out)
}

fn repair_args_array_inner(inner: &str) -> Option<String> {
    let mut changed = false;
    let mut out_parts: Vec<String> = Vec::new();
    for part in inner.split(',') {
        let token = part.trim();
        if token.is_empty() {
            out_parts.push(part.to_string());
            continue;
        }
        let keep_as_is = token.starts_with('"')
            || token.starts_with('{')
            || token.starts_with('[')
            || token == "true"
            || token == "false"
            || token == "null"
            || token.parse::<f64>().is_ok();
        if keep_as_is {
            out_parts.push(token.to_string());
            continue;
        }
        if token
            .chars()
            .all(|c| !c.is_control() && c != '"' && c != '\\')
        {
            changed = true;
            out_parts.push(format!("\"{token}\""));
        } else {
            out_parts.push(token.to_string());
        }
    }
    changed.then(|| out_parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{parse_args_json, try_repair_run_command_args_json};

    #[test]
    fn ok_object() {
        let v = parse_args_json(r#"{"a":1}"#).expect("valid");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn err_prefix() {
        let e = parse_args_json("{").unwrap_err();
        assert!(e.starts_with("参数 JSON 无效:"), "got {e:?}");
    }

    #[test]
    fn repair_run_command_args_bare_token() {
        let raw = r#"{"command":"ls","args":[-la]}"#;
        let repaired =
            try_repair_run_command_args_json(raw).expect("should repair run_command args array");
        let parsed = serde_json::from_str::<serde_json::Value>(&repaired).expect("valid json");
        assert_eq!(parsed["args"][0], "-la");
    }
}
