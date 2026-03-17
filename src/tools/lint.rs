use std::path::Path;
use std::process::Command;

/// 运行 cargo clippy 和（可选）frontend 的 npm lint，将结果聚合为一段文本。
///
/// 参数 JSON:
/// {
///   "run_cargo": bool,        // 可选，默认 true
///   "run_frontend": bool      // 可选，默认 true（仅在 frontend 目录存在且有 package.json 时尝试）
/// }
pub fn run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let run_cargo = v
        .get("run_cargo")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let run_frontend = v
        .get("run_frontend")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);

    let mut sections = Vec::new();

    if run_cargo {
        sections.push(run_cargo_clippy(workspace_root, max_output_len));
    }
    if run_frontend {
        sections.push(run_frontend_lint(workspace_root, max_output_len));
    }

    sections.join("\n\n====================\n\n").trim().to_string()
}

fn run_cargo_clippy(root: &Path, max_output_len: usize) -> String {
    let dir = root.to_path_buf();
    // 假设后端 crate 就在工作区根目录（即当前项目），找不到也尽量执行
    if !dir.join("Cargo.toml").is_file() {
        return "cargo clippy: 跳过（未找到 Cargo.toml）".to_string();
    }
    let output = match Command::new("cargo")
        .arg("clippy")
        .arg("--all-targets")
        .arg("--locked")
        .current_dir(&dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return format!("cargo clippy: 无法启动命令（{}）", e);
        }
    };
    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut s = String::new();
    s.push_str(&format!(
        "cargo clippy (exit={}):\n",
        status.code().unwrap_or(-1)
    ));
    let body = if !stderr.is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };
    s.push_str(&truncate(&body, max_output_len));
    s
}

fn run_frontend_lint(root: &Path, max_output_len: usize) -> String {
    let frontend = root.join("frontend");
    if !frontend.is_dir() {
        return "npm run lint: 跳过（frontend 目录不存在）".to_string();
    }
    if !frontend.join("package.json").is_file() {
        return "npm run lint: 跳过（未找到 package.json）".to_string();
    }
    let output = match Command::new("npm")
        .arg("run")
        .arg("lint")
        .current_dir(&frontend)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return format!("npm run lint: 无法启动命令（{}）", e);
        }
    };
    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut s = String::new();
    s.push_str(&format!(
        "npm run lint (exit={}):\n",
        status.code().unwrap_or(-1)
    ));
    let body = if !stderr.is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };
    s.push_str(&truncate(&body, max_output_len));
    s
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut out = s[..max_bytes].to_string();
    out.push_str("\n\n... (lint 输出已截断)");
    out
}

