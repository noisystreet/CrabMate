//! 在工作目录下执行可执行程序
//!
//! 路径为相对于工作目录的相对路径，且不能通过 .. 超出工作目录。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
fn resolve_executable_path(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))?;
    let joined = base_canonical.join(sub);
    let normalized = normalize_path(&joined);
    if !normalized.starts_with(&base_canonical) {
        return Err("路径不能超出工作目录".to_string());
    }
    Ok(normalized)
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
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
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let exec_path = match args.get("path").and_then(|p| p.as_str()) {
        Some(s) => s,
        None => return "错误：缺少 path 参数".to_string(),
    };
    let target = match resolve_executable_path(working_dir, exec_path) {
        Ok(p) => p,
        Err(e) => return e,
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
    let truncate = |s: &str| truncate_output(s, max_output_len);
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

fn truncate_output(s: &str, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= MAX_OUTPUT_LINES && s.len() <= max_bytes {
        return s.to_string();
    }
    let kept_lines = lines.len().min(MAX_OUTPUT_LINES);
    let kept = lines[..kept_lines].join("\n");
    let kept = if kept.len() <= max_bytes {
        kept
    } else {
        truncate_to_char_boundary(&kept, max_bytes)
    };
    let total_lines = lines.len();
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        kept, kept_lines, total_lines
    )
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn format_spawn_error(_target: &Path, e: &io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => "错误：目标可执行文件不存在或不可访问（可能在执行前被删除）".to_string(),
        PermissionDenied => "错误：缺少执行该文件的权限（请检查文件权限或挂载选项）".to_string(),
        _ => format!("错误：无法启动可执行文件（系统错误：{}）", e),
    }
}
