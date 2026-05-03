//! 容器 CLI 最小封装：Docker / Podman / docker compose（只读或受控构建参数）。

use std::path::Path;
use std::process::Command;

use super::output_util;
use super::tool_param_types::{DockerBuildArgs, DockerComposePsArgs, PodmanImagesArgs};

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

fn run_and_format(cmd: Command, max_output_len: usize, title: &str) -> String {
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
        output_util::CommandSpawnErrorStyle::CannotStartCommand,
    )
}

pub fn docker_build(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: DockerBuildArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 docker_build 形状不一致: {e}"),
    };

    let context = args
        .context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if let Err(e) = safe_relative_path(context, "context") {
        return format!("错误：{}", e);
    }

    let tag = args
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("crabmate-local:latest");
    if let Err(e) = safe_image_ref(tag) {
        return format!("错误：{}", e);
    }

    if let Some(f) = args
        .dockerfile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && let Err(e) = safe_relative_path(f, "dockerfile")
    {
        return format!("错误：{}", e);
    }

    let no_cache = args.no_cache;

    let mut cmd = Command::new("docker");
    cmd.arg("build").arg("-t").arg(tag);
    if no_cache {
        cmd.arg("--no-cache");
    }
    if let Some(f) = args
        .dockerfile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        cmd.arg("-f").arg(f);
    }
    cmd.arg(context);
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "docker build")
}

pub fn docker_compose_ps(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: DockerComposePsArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 docker_compose_ps 形状不一致: {e}"),
    };

    if let Some(p) = args
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && let Err(e) = safe_compose_project(p)
    {
        return format!("错误：{}", e);
    }

    for f in &args.compose_files {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        if let Err(e) = safe_relative_path(f, "compose_files") {
            return format!("错误：{}", e);
        }
    }

    let mut cmd = Command::new("docker");
    cmd.arg("compose");
    if let Some(p) = args
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        cmd.arg("-p").arg(p);
    }
    for f in &args.compose_files {
        let t = f.trim();
        if !t.is_empty() {
            cmd.arg("-f").arg(t);
        }
    }
    cmd.arg("ps");
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "docker compose ps")
}

pub fn podman_images(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: PodmanImagesArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 podman_images 形状不一致: {e}"),
    };

    let reference = args
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(r) = reference
        && let Err(e) = safe_image_ref(r)
    {
        return format!("错误：{}", e);
    }

    let mut cmd = Command::new("podman");
    cmd.arg("images");
    if let Some(r) = reference {
        cmd.arg(r);
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "podman images")
}
