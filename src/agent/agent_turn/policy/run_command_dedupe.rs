//! `run_command` 签名规范化与同轮精确去重（外循环完成抑制 + 单轮成功缓存共用）。

use std::collections::HashSet;
use std::path::Path;

use crate::cm_tools::tools::CommandDigest;
use crate::cm_types::{Message, message_content_as_str};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCommandInvocation {
    pub command: String,
    pub args: Vec<String>,
}

/// 宽松解析：仅用于规范化去重 key（不做执行裁定）。
///
/// 与执行路径的 [`crate::cm_tools::tools::parse_command_spec`] 刻意解耦：这里容忍 `args` 非数组
/// （视为空）并过滤非字符串元素，使历史消息与不规范入参也能得到稳定 key。
pub fn parse_run_command_args(args_json: &str) -> Option<RunCommandInvocation> {
    let (command, args) = crate::cm_tools::tools::parse_run_command_payload_lenient(args_json)?;
    if command.is_empty() {
        return None;
    }
    Some(RunCommandInvocation {
        command,
        args: args.into_iter().map(|s| s.trim().to_string()).collect(),
    })
}

/// 规范化 `run_command` 签名；显式 `cwd` 字段也会纳入 key（与 `cd` 前缀语义对齐）。
pub fn normalize_run_command_key(args_json: &str) -> Option<String> {
    let inv = parse_run_command_args(args_json)?;
    let digest = CommandDigest::from_invocation(inv.command, inv.args);
    Some(
        match explicit_cwd_from_args(args_json) {
            Some(cwd) => digest.with_cwd(Path::new(&cwd)),
            None => digest,
        }
        .key(),
    )
}

fn explicit_cwd_from_args(args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    v.get("cwd")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 与 [`normalize_run_command_key`] 相同，但把执行目录纳入 key。
pub fn normalize_run_command_key_with_cwd(args_json: &str, cwd: &Path) -> Option<String> {
    let inv = parse_run_command_args(args_json)?;
    Some(
        CommandDigest::from_invocation(inv.command, inv.args)
            .with_cwd(cwd)
            .key(),
    )
}

/// 单轮内可对已成功执行过的 `run_command` 做精确签名去重（不限工具链）。
#[cfg(test)]
pub fn run_command_duplicate_suppress_key(args_json: &str) -> Option<String> {
    normalize_run_command_key(args_json)
}

/// 单轮成功缓存使用的 key：同一命令在不同 `cwd` 下视为不同调用。
pub fn run_command_duplicate_suppress_key_with_cwd(args_json: &str, cwd: &Path) -> Option<String> {
    normalize_run_command_key_with_cwd(args_json, cwd)
}

fn tool_exit_ok_from_raw(raw: &str) -> bool {
    if let Some(env) = crate::cm_tools::tool_result::normalize_tool_message_content(raw) {
        return env.ok || env.exit_code == Some(0);
    }
    let parsed = crate::cm_tools::tool_result::parse_tool_output("run_command", raw);
    parsed.ok || parsed.exit_code == Some(0)
}

fn run_command_args_from_tool_message(raw: &str) -> Option<String> {
    if let Some(env) = crate::cm_tools::tool_result::normalize_tool_message_content(raw) {
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
pub fn successful_run_command_keys_from_messages(messages: &[Message]) -> HashSet<String> {
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
                infer_args_json_from_run_output(raw)
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

/// 仅用于**历史消息回放**：旧版工具输出没有结构化 `args_json`，只能从 `命令：…` / `$ …` 文本反推。
///
/// 禁止新代码依赖此函数；新输出必须经 [`run_command_args_from_tool_message`] 读取结构化字段。
/// 保留它是为了让跨版本的历史会话仍能参与同轮去重。
fn infer_args_json_from_run_output(raw: &str) -> Option<String> {
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

pub const RUN_COMMAND_DUPLICATE_SUPPRESSED_MSG: &str =
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

    #[test]
    fn normalize_key_exact_shape() {
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cmake","args":["--build","build"]}"#).as_deref(),
            Some("run_command|cmake|--build\u{1f}build")
        );
    }

    #[test]
    fn normalize_key_trims_command_and_args() {
        assert_eq!(
            normalize_run_command_key(
                r#"{"command":"  cmake  ","args":[" --build "," build "],"other":1}"#
            )
            .as_deref(),
            Some("run_command|cmake|--build\u{1f}build")
        );
    }

    #[test]
    fn normalize_key_defaults_missing_or_non_array_args_to_empty() {
        let expected = Some("run_command|cmake|");
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cmake"}"#).as_deref(),
            expected
        );
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cmake","args":"--build"}"#).as_deref(),
            expected
        );
    }

    #[test]
    fn normalize_key_filters_non_string_args() {
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cmake","args":["--build",1,null,"build"]}"#)
                .as_deref(),
            Some("run_command|cmake|--build\u{1f}build")
        );
    }

    #[test]
    fn normalize_key_rejects_unparsable_or_empty_command() {
        assert!(normalize_run_command_key("not json").is_none());
        assert!(normalize_run_command_key(r#"{"args":["x"]}"#).is_none());
        assert!(normalize_run_command_key(r#"{"command":"   ","args":[]}"#).is_none());
    }

    #[test]
    fn normalize_key_with_cwd_extends_legacy_shape() {
        let key = normalize_run_command_key_with_cwd(
            r#"{"command":"cmake","args":["--build","build"]}"#,
            Path::new("/tmp/ws"),
        )
        .expect("key");
        assert!(
            key.starts_with("run_command|cmake|--build\u{1f}build|meta="),
            "{key}"
        );
        assert!(key.contains(r#""cwd":"/tmp/ws""#), "{key}");
    }

    #[test]
    fn normalize_key_includes_explicit_cwd_field() {
        let a = normalize_run_command_key(r#"{"command":"cargo","args":["test"],"cwd":"a"}"#)
            .expect("key");
        let b = normalize_run_command_key(r#"{"command":"cargo","args":["test"],"cwd":"b"}"#)
            .expect("key");
        assert_ne!(a, b);
        assert!(a.contains(r#""cwd":"a""#), "{a}");
        assert!(b.contains(r#""cwd":"b""#), "{b}");
    }

    #[test]
    fn normalize_key_ignores_blank_or_non_string_cwd() {
        let base = normalize_run_command_key(r#"{"command":"cargo","args":["test"]}"#);
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cargo","args":["test"],"cwd":"  "}"#),
            base
        );
        assert_eq!(
            normalize_run_command_key(r#"{"command":"cargo","args":["test"],"cwd":5}"#),
            base
        );
    }

    #[test]
    fn normalize_key_with_cwd_distinguishes_working_dir() {
        let args = r#"{"command":"cargo","args":["test"]}"#;
        let a = normalize_run_command_key_with_cwd(args, Path::new("/tmp/a"));
        let b = normalize_run_command_key_with_cwd(args, Path::new("/tmp/b"));
        assert_ne!(a, b);
    }

    #[test]
    fn normalize_key_with_cwd_keeps_shell_invocation_but_no_extra_meta() {
        let no_cwd = normalize_run_command_key(r#"{"command":"bash","args":["-c","echo hi"]}"#)
            .expect("key");
        assert_eq!(no_cwd, "run_command|bash|-c\u{1f}echo hi");

        let with_cwd =
            normalize_run_command_key_with_cwd(r#"{"command":"bash","args":["-c","echo hi"]}"#, Path::new("."))
                .expect("key");
        assert!(!with_cwd.contains(r#""shell""#), "{with_cwd}");
        assert!(!with_cwd.contains(r#""script""#), "{with_cwd}");
        assert!(with_cwd.contains(r#""cwd":".""#), "{with_cwd}");
    }
}
