//! 前端开发工具：frontend lint（结构化参数）

use std::path::Path;
use std::process::Command;

use super::output_util;

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
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
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
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
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

fn run_and_format(cmd: Command, max_output_len: usize, title: &str) -> String {
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::StderrElseStdout,
        output_util::CommandSpawnErrorStyle::CannotStartCommand,
    )
}
