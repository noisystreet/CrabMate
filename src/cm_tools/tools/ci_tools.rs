//! 本地 CI 辅助：cargo fmt --check

use std::path::Path;

fn cargo_fmt_check(workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_root.join("Cargo.toml").is_file() {
        return "cargo fmt --check: 跳过（未找到 Cargo.toml）".to_string();
    }
    let out = std::process::Command::new("cargo")
        .arg("fmt")
        .arg("--")
        .arg("--check")
        .current_dir(workspace_root)
        .output();
    match out {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let body = if !stderr.trim().is_empty() {
                stderr.to_string()
            } else if !stdout.trim().is_empty() {
                stdout.to_string()
            } else {
                "(无输出)".to_string()
            };
            let mut s = body;
            if s.len() > max_output_len {
                let truncated = super::output_util::truncate_to_char_boundary(&s, max_output_len);
                s = format!("{}\n\n... (输出已截断)", truncated);
            }
            format!("cargo fmt --check (exit={}):\n{}", status, s)
        }
        Err(e) => format!("cargo fmt --check: 执行失败（{}）", e),
    }
}

pub fn cargo_fmt_check_tool(
    _args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    cargo_fmt_check(workspace_root, max_output_len)
}
