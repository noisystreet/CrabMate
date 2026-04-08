//! 在工作目录下执行可执行程序
//!
//! 路径为相对于工作目录的相对路径，且不能通过 .. 超出工作目录。

use crate::path_workspace::{
    WorkspacePathError, absolutize_relative_under_root, canonical_workspace_root,
    ensure_existing_ancestor_within_root,
};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::output_util;

/// 简单的每秒执行限流状态
struct RateLimitState {
    window_sec: u64,
    count: u32,
}

/// 在任意 1 秒窗口内最多允许执行的可执行程序次数
const MAX_EXEC_PER_SEC: u32 = 5;

const MAX_OUTPUT_LINES: usize = 500;

static RATE_LIMIT: Mutex<RateLimitState> = Mutex::new(RateLimitState {
    window_sec: 0,
    count: 0,
});

/// 解析相对工作目录的路径，且不允许超出工作目录
fn resolve_executable_path(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if Path::new(sub).is_absolute() {
        return Err(WorkspacePathError::AbsolutePathNotAllowed);
    }
    let base_canonical = canonical_workspace_root(base)?;
    let normalized = absolutize_relative_under_root(&base_canonical, sub)?;
    ensure_existing_ancestor_within_root(&base_canonical, &normalized)?;
    Ok(normalized)
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

/// 在工作目录下执行指定可执行程序。args_json 中 path 为相对工作目录的路径，args 为参数列表。
pub fn run(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let args = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let exec_path = match args.get("path").and_then(|p| p.as_str()) {
        Some(s) => s,
        None => return "错误：缺少 path 参数".to_string(),
    };
    let target = match resolve_executable_path(working_dir, exec_path) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e.user_message()),
    };
    if !target.exists() {
        // 不回显完整内部路径，避免在多租户环境下泄露目录结构
        return "错误：指定的可执行路径不存在或不可访问".to_string();
    }
    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(_) => {
            // 同样不暴露底层错误细节与路径
            return "错误：无法读取可执行文件信息".to_string();
        }
    };
    if !is_executable(&meta) {
        return "错误：目标不是可执行文件或缺少执行权限".to_string();
    }
    let exec_args: Vec<String> = match args.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => return "错误：args 必须是字符串数组".to_string(),
        None => vec![],
    };
    for a in &exec_args {
        if a.contains("..") || a.trim_start().starts_with('/') {
            return "错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）".to_string();
        }
    }
    // 简单每秒限流，防止高频滥用可执行程序
    if let Err(e) = check_rate_limit() {
        return e;
    }
    let output = match std::process::Command::new(&target)
        .args(&exec_args)
        .current_dir(working_dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format_spawn_error(&target, &e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let truncate =
        |s: &str| output_util::truncate_output_lines(s, max_output_len, MAX_OUTPUT_LINES);
    let status = output.status;
    let mut out = format!("退出码：{}\n", status.code().unwrap_or(-1));
    if !stdout.is_empty() {
        out.push_str("标准输出：\n");
        out.push_str(&truncate(&stdout));
    }
    if !stderr.is_empty() {
        out.push_str("标准错误：\n");
        out.push_str(&truncate(&stderr));
    }
    if stdout.is_empty() && stderr.is_empty() && status.success() {
        out.push_str("(无输出)");
    }
    out.trim_end().to_string()
}

fn now_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn check_rate_limit() -> Result<(), String> {
    let mut state = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_sec();
    if now != state.window_sec {
        state.window_sec = now;
        state.count = 0;
    }
    if state.count >= MAX_EXEC_PER_SEC {
        return Err(format!(
            "可执行程序调用过于频繁：每秒最多允许 {} 次，请稍后再试",
            MAX_EXEC_PER_SEC
        ));
    }
    state.count += 1;
    Ok(())
}

fn format_spawn_error(_target: &Path, e: &io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => "错误：目标可执行文件不存在或不可访问（可能在执行前被删除）".to_string(),
        PermissionDenied => "错误：缺少执行该文件的权限（请检查文件权限或挂载选项）".to_string(),
        _ => format!("错误：无法启动可执行文件（系统错误：{}）", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn make_test_dir() -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "crabmate_exec_tool_test_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_executable_path_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = make_test_dir();
        let outside = std::env::temp_dir().join(format!(
            "crabmate_exec_outside_{}_{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&outside).unwrap();
        let link = dir.join("bin");
        symlink(&outside, &link).unwrap();

        let res = resolve_executable_path(&dir, "bin/tool.sh");
        assert!(res.is_err(), "应拒绝 symlink 绕过执行路径");
        let msg = res.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("路径不能超出工作目录"),
            "报错应提示越界: {msg}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
