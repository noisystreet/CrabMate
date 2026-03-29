//! 容器 CLI 最小封装：Docker / Podman / docker compose（只读或受控构建参数）。

use std::path::Path;
use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 800;

fn safe_relative_path(rel: &str, label: &str) -> Result<(), String> {
    let t = rel.trim();
    if t.is_empty() {
        return Err(format!("{label} 不能为空"));
    }
    if t.contains("..") || t.starts_with('/') {
        return Err(format!("{label} 禁止 .. 与绝对路径"));
    }
    Ok(())
}

/// 镜像引用：常见 registry/tag 字符，禁止空白与控制字符。
fn safe_image_ref(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("镜像名不能为空".to_string());
    }
    if t.chars().any(|c| c.is_whitespace()) {
        return Err("镜像名不能含空白".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_' | '/' | '@'))
    {
        return Err("镜像名含非法字符".to_string());
    }
    Ok(())
}

fn safe_compose_project(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("project 不能为空".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("project 仅允许字母数字与 _-".to_string());
    }
    Ok(())
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut body = String::new();
            if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(stderr.trim_end());
            }
            if body.is_empty() {
                body.push_str("(无输出)");
            }
            format!(
                "{} (exit={}):\n{}",
                title,
                status,
                output_util::truncate_output_lines(&body, max_output_len, MAX_OUTPUT_LINES)
            )
        }
        Err(e) => format!("{}: 无法启动命令（{}）", title, e),
    }
}

pub fn docker_build(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };

    let context = v
        .get("context")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if let Err(e) = safe_relative_path(context, "context") {
        return format!("错误：{}", e);
    }

    let tag = v
        .get("tag")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("crabmate-local:latest");
    if let Err(e) = safe_image_ref(tag) {
        return format!("错误：{}", e);
    }

    if let Some(f) = v.get("dockerfile").and_then(|x| x.as_str())
        && !f.trim().is_empty()
        && let Err(e) = safe_relative_path(f.trim(), "dockerfile")
    {
        return format!("错误：{}", e);
    }

    let no_cache = v.get("no_cache").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("docker");
    cmd.arg("build").arg("-t").arg(tag);
    if no_cache {
        cmd.arg("--no-cache");
    }
    if let Some(f) = v.get("dockerfile").and_then(|x| x.as_str()).map(str::trim)
        && !f.is_empty()
    {
        cmd.arg("-f").arg(f);
    }
    cmd.arg(context);
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "docker build")
}

pub fn docker_compose_ps(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };

    if let Some(p) = v.get("project").and_then(|x| x.as_str())
        && !p.trim().is_empty()
        && let Err(e) = safe_compose_project(p.trim())
    {
        return format!("错误：{}", e);
    }

    if let Some(arr) = v.get("compose_files").and_then(|x| x.as_array()) {
        for x in arr {
            let Some(f) = x.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            if let Err(e) = safe_relative_path(f, "compose_files") {
                return format!("错误：{}", e);
            }
        }
    }

    let mut cmd = Command::new("docker");
    cmd.arg("compose");
    if let Some(p) = v.get("project").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
    {
        cmd.arg("-p").arg(p);
    }
    if let Some(arr) = v.get("compose_files").and_then(|x| x.as_array()) {
        for x in arr {
            if let Some(f) = x.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                cmd.arg("-f").arg(f);
            }
        }
    }
    cmd.arg("ps");
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "docker compose ps")
}

pub fn podman_images(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };

    let reference = v.get("reference").and_then(|x| x.as_str()).map(str::trim);
    if let Some(r) = reference.filter(|s| !s.is_empty())
        && let Err(e) = safe_image_ref(r)
    {
        return format!("错误：{}", e);
    }

    let mut cmd = Command::new("podman");
    cmd.arg("images");
    if let Some(r) = reference.filter(|s| !s.is_empty()) {
        cmd.arg(r);
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "podman images")
}
