//! Go 语言工具链：go build / test / vet / mod tidy / fmt check

use std::path::Path;
use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 800;

fn has_go_project(workspace_root: &Path) -> bool {
    workspace_root.join("go.mod").is_file()
}

pub fn go_build(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "go build: 跳过（未找到 go.mod）".to_string();
    }

    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let race = v.get("race").and_then(|x| x.as_bool()).unwrap_or(false);
    let verbose = v.get("verbose").and_then(|x| x.as_bool()).unwrap_or(false);
    let tags = v
        .get("tags")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut cmd = Command::new("go");
    cmd.arg("build");
    if race {
        cmd.arg("-race");
    }
    if verbose {
        cmd.arg("-v");
    }
    if let Some(t) = tags {
        cmd.arg("-tags").arg(t);
    }
    cmd.arg(package).current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "go build")
}

pub fn go_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "go test: 跳过（未找到 go.mod）".to_string();
    }

    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let run_filter = v
        .get("run")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let race = v.get("race").and_then(|x| x.as_bool()).unwrap_or(false);
    let verbose = v.get("verbose").and_then(|x| x.as_bool()).unwrap_or(true);
    let short = v.get("short").and_then(|x| x.as_bool()).unwrap_or(false);
    let count = v.get("count").and_then(|x| x.as_u64());
    let timeout = v
        .get("timeout")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tags = v
        .get("tags")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut cmd = Command::new("go");
    cmd.arg("test");
    if verbose {
        cmd.arg("-v");
    }
    if race {
        cmd.arg("-race");
    }
    if short {
        cmd.arg("-short");
    }
    if let Some(r) = run_filter {
        cmd.arg("-run").arg(r);
    }
    if let Some(c) = count {
        cmd.arg("-count").arg(c.to_string());
    }
    if let Some(t) = timeout {
        cmd.arg("-timeout").arg(t);
    }
    if let Some(t) = tags {
        cmd.arg("-tags").arg(t);
    }
    cmd.arg(package).current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "go test")
}

pub fn go_vet(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "go vet: 跳过（未找到 go.mod）".to_string();
    }

    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let tags = v
        .get("tags")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut cmd = Command::new("go");
    cmd.arg("vet");
    if let Some(t) = tags {
        cmd.arg("-tags").arg(t);
    }
    cmd.arg(package).current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "go vet")
}

pub fn go_mod_tidy(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "go mod tidy: 跳过（未找到 go.mod）".to_string();
    }
    let confirm = v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false);
    if !confirm {
        return "拒绝执行：go_mod_tidy 需要 confirm=true".to_string();
    }

    let mut cmd = Command::new("go");
    cmd.arg("mod").arg("tidy").current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "go mod tidy")
}

pub fn go_fmt_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "gofmt: 跳过（未找到 go.mod）".to_string();
    }

    let path = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if path.contains("..") || path.starts_with('/') {
        return "错误：path 参数不安全".to_string();
    }

    let mut cmd = Command::new("gofmt");
    cmd.arg("-l").arg(path).current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "gofmt -l（列出未格式化文件）")
}

pub fn golangci_lint(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_go_project(workspace_root) {
        return "golangci-lint: 跳过（未找到 go.mod）".to_string();
    }

    let fix = v.get("fix").and_then(|x| x.as_bool()).unwrap_or(false);
    let fast = v.get("fast").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("golangci-lint");
    cmd.arg("run");
    if fix {
        cmd.arg("--fix");
    }
    if fast {
        cmd.arg("--fast");
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "golangci-lint run")
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
