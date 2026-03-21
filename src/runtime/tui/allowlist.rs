//! TUI 命令永久允许列表（落盘 JSON）。

use std::collections::HashSet;
use std::path::Path;

pub(super) fn command_approval_message(command: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("命令审批：{}", command)
    } else {
        format!("命令审批：{} {}", command, args)
    }
}

pub(super) fn load_persistent_allowlist(path: &Path) -> HashSet<String> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(_) => return HashSet::new(),
    };
    v.get("commands")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn save_persistent_allowlist(path: &Path, allowlist: &HashSet<String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut items = allowlist.iter().cloned().collect::<Vec<_>>();
    items.sort();
    let body = serde_json::json!({ "commands": items });
    if let Ok(s) = serde_json::to_string_pretty(&body) {
        let _ = std::fs::write(path, s.as_bytes());
    }
}
