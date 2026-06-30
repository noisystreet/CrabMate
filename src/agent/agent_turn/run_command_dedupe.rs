//! `run_command` 签名规范化与同轮精确去重（外循环完成抑制 + 单轮成功缓存共用）。

use std::collections::HashSet;

use crate::types::{Message, message_content_as_str};

const KEY_SEP: &str = "\u{1f}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunCommandInvocation {
    pub command: String,
    pub args: Vec<String>,
}

pub(crate) fn parse_run_command_args(args_json: &str) -> Option<RunCommandInvocation> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_string();
    if command.is_empty() {
        return None;
    }
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(RunCommandInvocation { command, args })
}

pub(crate) fn normalize_run_command_key(args_json: &str) -> Option<String> {
    let inv = parse_run_command_args(args_json)?;
    Some(format!(
        "run_command|{}|{}",
        inv.command,
        inv.args.join(KEY_SEP)
    ))
}

/// 单轮内可对已成功执行过的 `run_command` 做精确签名去重（不限工具链）。
pub(crate) fn run_command_duplicate_suppress_key(args_json: &str) -> Option<String> {
    normalize_run_command_key(args_json)
}

fn tool_exit_ok_from_raw(raw: &str) -> bool {
    if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
        return env.ok || env.exit_code == Some(0);
    }
    let parsed = crate::tool_result::parse_legacy_output("run_command", raw);
    parsed.ok || parsed.exit_code == Some(0)
}

fn run_command_args_from_tool_message(raw: &str) -> Option<String> {
    if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
        if env.name != "run_command" {
            return None;
        }
        return env.structured_payload.as_ref().and_then(|v| {
            v.get("args_json")
                .and_then(|x| x.as_str())
                .map(str::to_string)
                .or_else(|| {
                    v.get("command").and_then(|c| c.as_str()).map(|command| {
                        let args = v
                            .get("args")
                            .and_then(|a| a.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
                            .unwrap_or_default();
                        serde_json::json!({ "command": command, "args": args }).to_string()
                    })
                })
        });
    }
    None
}

/// 自消息历史收集已成功执行过的 `run_command` 规范化签名。
pub(crate) fn successful_run_command_keys_from_messages(messages: &[Message]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for m in messages {
        if m.role != "tool" && m.tool_call_id.is_none() {
            continue;
        }
        let Some(raw) = message_content_as_str(&m.content) else {
            continue;
        };
        if !tool_exit_ok_from_raw(raw) {
            continue;
        }
        let args_json = run_command_args_from_tool_message(raw).or_else(|| {
            if raw.contains("命令：") || raw.starts_with('$') {
                infer_args_json_from_legacy_run_output(raw)
            } else {
                None
            }
        });
        if let Some(args_json) = args_json
            && let Some(key) = normalize_run_command_key(&args_json)
        {
            keys.insert(key);
        }
    }
    keys
}

fn infer_args_json_from_legacy_run_output(raw: &str) -> Option<String> {
    let line = raw
        .lines()
        .find(|l| l.starts_with('$') || l.starts_with("命令："))?;
    let invocation = line
        .trim_start_matches('$')
        .trim_start_matches("命令：")
        .trim();
    if invocation.is_empty() {
        return None;
    }
    let mut parts = invocation.split_whitespace();
    let command = parts.next()?.to_string();
    let args: Vec<String> = parts.map(str::to_string).collect();
    Some(serde_json::json!({ "command": command, "args": args }).to_string())
}

pub(crate) const RUN_COMMAND_DUPLICATE_SUPPRESSED_MSG: &str =
    "命令重复执行已抑制：本轮内已成功执行过相同命令";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_suppress_key_matches_normalize_for_run_command() {
        let args = r#"{"command":"cmake","args":["--build","build"]}"#;
        assert_eq!(
            run_command_duplicate_suppress_key(args),
            normalize_run_command_key(args)
        );
    }

    #[test]
    fn normalize_key_stable_for_same_invocation() {
        let a = normalize_run_command_key(r#"{"command":"cmake","args":["--build","build"]}"#);
        let b = normalize_run_command_key(r#"{"command":"cmake","args":["--build","build"]}"#);
        assert_eq!(a, b);
    }

    #[test]
    fn different_invocations_have_different_keys() {
        let a = normalize_run_command_key(r#"{"command":"cmake","args":["--build","build"]}"#);
        let b = normalize_run_command_key(r#"{"command":"./build/hello","args":[]}"#);
        assert_ne!(a, b);
    }
}
