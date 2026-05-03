//! Go 语言工具链：go build / test / vet / mod tidy / fmt check

use std::path::Path;
use std::process::Command;

use super::output_util;
use super::tool_param_types::{
    GoBuildArgs, GoFmtCheckArgs, GoModTidyArgs, GoTestArgs, GoVetArgs, GolangciLintArgs,
};

const MAX_OUTPUT_LINES: usize = 800;

fn has_go_project(workspace_root: &Path) -> bool {
    workspace_root.join("go.mod").is_file()
}

pub fn go_build(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: GoBuildArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 go_build 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "go build: 跳过（未找到 go.mod）".to_string();
    }

    let package = args
        .package
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let race = args.race;
    let verbose = args.verbose;
    let tags = args
        .tags
        .as_deref()
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
    if let Some(out) = args
        .output
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if out.contains("..") || out.starts_with('/') {
            return "错误：output 参数不安全".to_string();
        }
        cmd.arg("-o").arg(out);
    }
    cmd.arg(package).current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "go build")
}

pub fn go_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: GoTestArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 go_test 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "go test: 跳过（未找到 go.mod）".to_string();
    }

    let package = args
        .package
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let run_filter = args.run.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let race = args.race;
    let verbose = args.verbose;
    let short = args.short;
    let count = args.count;
    let timeout = args
        .timeout
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tags = args
        .tags
        .as_deref()
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
    let args: GoVetArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 go_vet 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "go vet: 跳过（未找到 go.mod）".to_string();
    }

    let package = args
        .package
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("./...");
    if package.contains("..") && package != "./..." && package != "..." {
        return "错误：package 参数不安全".to_string();
    }
    let tags = args
        .tags
        .as_deref()
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
    let args: GoModTidyArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 go_mod_tidy 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "go mod tidy: 跳过（未找到 go.mod）".to_string();
    }
    if !args.confirm {
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
    let args: GoFmtCheckArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 go_fmt_check 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "gofmt: 跳过（未找到 go.mod）".to_string();
    }

    let path = args
        .path
        .as_deref()
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
    let GolangciLintArgs { fix, fast } = match serde_json::from_value::<GolangciLintArgs>(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 golangci_lint 形状不一致: {e}"),
    };
    if !has_go_project(workspace_root) {
        return "golangci-lint: 跳过（未找到 go.mod）".to_string();
    }

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
