//! 进程内、按工作区键隔离的只读类 **`run_command`** 短时结果缓存（TTL + 容量上限）。
//!
//! 用于减少模型在短时间内重复发起同一探测命令带来的 token 与子进程开销。**不会**缓存失败输出；
//! 任意非只读工具或本轮 **`workspace_changed`** 时对该工作区键整组失效（与 **`ReadFileTurnCache`** 策略对齐）。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn parse_run_command_payload(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_string();
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some((command, args))
}

const KEY_SEP: char = '\x1f';

/// 与 [`Self::invalidate_workspace`] 使用的键前缀一致：`{workspace}{KEY_SEP}`。
#[inline]
fn composite_key(workspace_key: &str, tool: &str, args: &str) -> String {
    format!("{workspace_key}{KEY_SEP}{tool}{KEY_SEP}{args}")
}

#[derive(Clone)]
struct CacheEntry {
    inserted_at: Instant,
    expires_at: Instant,
    output: String,
}

/// 进程级句柄：挂在 [`crate::process_handles::ProcessHandles`] 上，由 Web / CLI 共用。
pub struct ReadonlyToolTtlCache {
    inner: Mutex<HashMap<String, CacheEntry>>,
}

impl ReadonlyToolTtlCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn try_get(&self, workspace_key: &str, tool: &str, args: &str) -> Option<String> {
        let key = composite_key(workspace_key, tool, args);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let e = g.get(&key)?;
        if Instant::now() >= e.expires_at {
            g.remove(&key);
            return None;
        }
        Some(e.output.clone())
    }

    pub fn insert(
        &self,
        workspace_key: &str,
        tool: &str,
        args: &str,
        output: String,
        ttl: Duration,
        max_entries: usize,
    ) {
        let max_entries = max_entries.max(1);
        let now = Instant::now();
        let key = composite_key(workspace_key, tool, args);
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.insert(
            key,
            CacheEntry {
                inserted_at: now,
                expires_at: now + ttl,
                output,
            },
        );
        while g.len() > max_entries {
            let victim = g
                .iter()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(k, _)| k.clone());
            let Some(k) = victim else {
                break;
            };
            g.remove(&k);
        }
    }

    pub fn remove(&self, workspace_key: &str, tool: &str, args: &str) {
        let key = composite_key(workspace_key, tool, args);
        if let Ok(mut g) = self.inner.lock() {
            g.remove(&key);
        }
    }

    /// 移除某工作区下的全部条目（键前缀 `{workspace_key}\x1f`）。
    pub fn invalidate_workspace(&self, workspace_key: &str) {
        let prefix = format!("{workspace_key}{KEY_SEP}");
        if let Ok(mut g) = self.inner.lock() {
            g.retain(|k, _| !k.starts_with(&prefix));
        }
    }
}

impl Default for ReadonlyToolTtlCache {
    fn default() -> Self {
        Self::new()
    }
}

fn args_forbid_absolute_or_dotdot(args: &[String]) -> bool {
    args.iter().any(|a| a.contains("..") || a.starts_with('/'))
}

fn first_git_subcommand(args: &[String]) -> Option<&str> {
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-C" | "--work-tree" | "--git-dir" => {
                i = i.saturating_add(2);
            }
            _ if a.starts_with('-') => {
                i = i.saturating_add(1);
            }
            _ => return Some(a),
        }
    }
    None
}

fn git_subcommand_ttl_eligible(sub: &str) -> bool {
    matches!(
        sub,
        "status"
            | "diff"
            | "log"
            | "show"
            | "branch"
            | "rev-parse"
            | "describe"
            | "ls-files"
            | "ls-tree"
            | "cat-file"
            | "blame"
            | "grep"
    )
}

fn cargo_first_subcommand_ttl_eligible(args: &[String]) -> bool {
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if a.starts_with('-') {
            i = i.saturating_add(1);
            continue;
        }
        return matches!(a, "metadata" | "tree" | "pkgid" | "locate-project");
    }
    false
}

fn standalone_ttl_eligible(cmd: &str) -> bool {
    matches!(
        cmd,
        "cat"
            | "head"
            | "tail"
            | "wc"
            | "stat"
            | "file"
            | "ls"
            | "pwd"
            | "echo"
            | "printf"
            | "dirname"
            | "basename"
            | "readlink"
            | "realpath"
            | "which"
            | "type"
            | "grep"
            | "egrep"
            | "fgrep"
            | "rg"
            | "jq"
            | "sort"
            | "uniq"
            | "cut"
            | "tr"
            | "column"
            | "cmp"
            | "diff"
            | "objdump"
            | "nm"
            | "readelf"
            | "strings"
            | "size"
            | "hexdump"
            | "ldd"
            | "od"
            | "xxd"
            | "zcat"
            | "c++filt"
    )
}

/// 是否可将本次 **`run_command`** 调用纳入 TTL 缓存（命中与写入前均须为真）。
///
/// 策略：拒绝 shell 解释器；**`git`** / **`cargo`** 仅允许保守子命令集；其余命令使用小型只读工具白名单；
/// 任意参数含 **`..`** 或以 **`/`** 开头时不缓存（与现有路径约束一致）。
pub fn run_command_invocation_ttl_cache_eligible(args_json: &str) -> bool {
    let Some((command, args)) = parse_run_command_payload(args_json) else {
        return false;
    };
    if command.trim().is_empty() || args_forbid_absolute_or_dotdot(&args) {
        return false;
    }
    let cmd = command.as_str();
    if matches!(
        cmd,
        "sh" | "bash" | "zsh" | "fish" | "dash" | "csh" | "tcsh" | "ksh" | "busybox"
    ) {
        return false;
    }
    match cmd {
        "git" => first_git_subcommand(&args).is_some_and(git_subcommand_ttl_eligible),
        "cargo" => cargo_first_subcommand_ttl_eligible(&args),
        _ => standalone_ttl_eligible(cmd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn args(cmd: &str, args: &[&str]) -> String {
        serde_json::json!({
            "command": cmd,
            "args": args,
        })
        .to_string()
    }

    #[test]
    fn eligibility_git_and_cargo() {
        assert!(run_command_invocation_ttl_cache_eligible(
            args("git", &["status"]).as_str()
        ));
        assert!(!run_command_invocation_ttl_cache_eligible(
            args("git", &["config", "--list"]).as_str()
        ));
        assert!(run_command_invocation_ttl_cache_eligible(
            args("cargo", &["metadata", "--no-deps", "--format-version", "1"]).as_str()
        ));
        assert!(!run_command_invocation_ttl_cache_eligible(
            args("cargo", &["check"]).as_str()
        ));
    }

    #[test]
    fn eligibility_denies_shell_and_absolute_args() {
        assert!(!run_command_invocation_ttl_cache_eligible(
            args("bash", &["-c", "echo hi"]).as_str()
        ));
        assert!(!run_command_invocation_ttl_cache_eligible(
            args("cat", &["/etc/passwd"]).as_str()
        ));
        assert!(!run_command_invocation_ttl_cache_eligible(
            args("cat", &["../x"]).as_str()
        ));
    }

    #[test]
    fn cache_expires_and_invalidates_workspace() {
        let c = ReadonlyToolTtlCache::new();
        let ws = "/tmp/ws";
        c.insert(
            ws,
            "run_command",
            r#"{"command":"echo","args":["x"]}"#,
            "out".into(),
            Duration::from_millis(80),
            8,
        );
        assert_eq!(
            c.try_get(ws, "run_command", r#"{"command":"echo","args":["x"]}"#)
                .as_deref(),
            Some("out")
        );
        thread::sleep(Duration::from_millis(120));
        assert!(
            c.try_get(ws, "run_command", r#"{"command":"echo","args":["x"]}"#)
                .is_none()
        );

        c.insert(
            ws,
            "run_command",
            r#"{"command":"echo","args":["a"]}"#,
            "a".into(),
            Duration::from_secs(60),
            8,
        );
        c.insert(
            ws,
            "run_command",
            r#"{"command":"echo","args":["b"]}"#,
            "b".into(),
            Duration::from_secs(60),
            8,
        );
        c.invalidate_workspace(ws);
        assert!(
            c.try_get(ws, "run_command", r#"{"command":"echo","args":["a"]}"#)
                .is_none()
        );
    }

    #[test]
    fn remove_single_entry() {
        let c = ReadonlyToolTtlCache::new();
        let ws = "w";
        let args_s = r#"{"command":"echo","args":["z"]}"#;
        c.insert(
            ws,
            "run_command",
            args_s,
            "z".into(),
            Duration::from_secs(30),
            8,
        );
        c.remove(ws, "run_command", args_s);
        assert!(c.try_get(ws, "run_command", args_s).is_none());
    }
}
