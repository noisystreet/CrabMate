//! 前端开发工具：frontend lint（结构化参数）

use std::path::Path;
use std::process::Command;

const MAX_OUTPUT_LINES: usize = 800;

pub fn frontend_lint(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    frontend_run_script(args_json, workspace_root, max_output_len, "lint")
}

pub fn frontend_build(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    frontend_run_script(args_json, workspace_root, max_output_len, "build")
}

pub fn frontend_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    frontend_run_script(args_json, workspace_root, max_output_len, "test")
}

/// `npx prettier --check .`（在指定前端子目录下），用于一致性检查而不改文件。
pub fn frontend_prettier_check(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = v
        .get("subdir")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("frontend");

    if subdir.starts_with('/') || subdir.contains("..") {
        return "错误：subdir 必须是工作区内相对路径，且不能包含 ..".to_string();
    }

    let dir = workspace_root.join(subdir);
    if !dir.is_dir() {
        return format!("prettier --check: 跳过（{} 目录不存在）", subdir);
    }
    if !dir.join("package.json").is_file() {
        return "prettier --check: 跳过（未找到 package.json）".to_string();
    }

    let mut cmd = Command::new("npx");
    cmd.arg("prettier")
        .arg("--check")
        .arg(".")
        .current_dir(&dir);
    run_and_format(
        cmd,
        max_output_len,
        &format!("npx prettier --check . (cwd={})", subdir),
    )
}

fn frontend_run_script(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    default_script: &str,
) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = v
        .get("subdir")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("frontend");
    let script = v
        .get("script")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_script);

    if subdir.starts_with('/') || subdir.contains("..") {
        return "错误：subdir 必须是工作区内相对路径，且不能包含 ..".to_string();
    }
    if script.contains(char::is_whitespace) {
        return "错误：script 参数无效（不能包含空白字符）".to_string();
    }

    let dir = workspace_root.join(subdir);
    if !dir.is_dir() {
        return format!("npm run {}: 跳过（{} 目录不存在）", script, subdir);
    }
    if !dir.join("package.json").is_file() {
        return format!("npm run {}: 跳过（未找到 package.json）", script);
    }

    let mut cmd = Command::new("npm");
    cmd.arg("run").arg(script).current_dir(&dir);
    run_and_format(cmd, max_output_len, &format!("npm run {}", script))
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut body = String::new();
            if !stderr.trim().is_empty() {
                body.push_str(stderr.trim_end());
            } else if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            } else {
                body.push_str("(无输出)");
            }
            format!(
                "{} (exit={}):\n{}",
                title,
                status,
                truncate_output(&body, max_output_len)
            )
        }
        Err(e) => format!("{}: 无法启动命令（{}）", title, e),
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if s.len() <= max_bytes && lines.len() <= MAX_OUTPUT_LINES {
        return s.to_string();
    }
    let kept_lines = lines.len().min(MAX_OUTPUT_LINES);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        joined[..max_bytes].to_string()
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}
