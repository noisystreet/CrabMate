//! ShellCheck 调用：路径收集与 CLI 装配。

use std::path::Path;
use std::process::Command;

use super::{MAX_PATHS, filter_existing, parse_rel_paths_from_slice, run_and_format};
use crate::tools::tool_param_types::{
    ShellcheckCheckArgs, ShellcheckOutputFormat, ShellcheckSeverity, ShellcheckShellDialect,
};

pub fn shellcheck_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: ShellcheckCheckArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 shellcheck_check 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("shellcheck");
    cmd.arg("--color=never").current_dir(&base);
    apply_shellcheck_optional_flags(&mut cmd, &args);

    let mut found_scripts = match collect_shellcheck_scripts(&base, &paths) {
        Ok(v) => v,
        Err(msg) => return msg,
    };
    const MAX_FILES: usize = 200;
    if found_scripts.len() > MAX_FILES {
        found_scripts.truncate(MAX_FILES);
    }
    for f in &found_scripts {
        cmd.arg(f);
    }
    run_and_format(cmd, max_output_len, "shellcheck")
}

fn apply_shellcheck_optional_flags(cmd: &mut Command, args: &ShellcheckCheckArgs) {
    if let Some(sev) = args.severity {
        let s = match sev {
            ShellcheckSeverity::Error => "error",
            ShellcheckSeverity::Warning => "warning",
            ShellcheckSeverity::Info => "info",
            ShellcheckSeverity::Style => "style",
        };
        cmd.arg("--severity").arg(s);
    }
    if let Some(sh) = args.shell {
        let s = match sh {
            ShellcheckShellDialect::Sh => "sh",
            ShellcheckShellDialect::Bash => "bash",
            ShellcheckShellDialect::Dash => "dash",
            ShellcheckShellDialect::Ksh => "ksh",
        };
        cmd.arg("--shell").arg(s);
    }
    if let Some(fmt) = args.format {
        let s = match fmt {
            ShellcheckOutputFormat::Tty => "tty",
            ShellcheckOutputFormat::Gcc => "gcc",
            ShellcheckOutputFormat::Json1 => "json1",
            ShellcheckOutputFormat::Checkstyle => "checkstyle",
            ShellcheckOutputFormat::Diff => "diff",
            ShellcheckOutputFormat::Quiet => "quiet",
        };
        cmd.arg("--format").arg(s);
    }
}

fn collect_shellcheck_scripts(base: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    let mut found_scripts = Vec::new();
    for p in paths {
        let full = base.join(p);
        if full.is_file() {
            found_scripts.push(p.clone());
        } else if full.is_dir()
            && let Ok(entries) = walkdir_shell_scripts(&full, base)
        {
            found_scripts.extend(entries);
        }
    }
    if found_scripts.is_empty() {
        return Err(
            "shellcheck: 在指定路径下未发现 shell 脚本（.sh/.bash/.zsh/.ksh 或含 shebang 的文件）"
                .to_string(),
        );
    }
    Ok(found_scripts)
}

fn walkdir_shell_scripts(dir: &Path, base: &Path) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    const SKIP_DIRS: &[&str] = &[
        "target",
        "node_modules",
        ".git",
        "vendor",
        "dist",
        "build",
        "__pycache__",
    ];
    while let Some(cur) = stack.pop() {
        let entries = match std::fs::read_dir(&cur) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name_str.as_ref()) && !name_str.starts_with('.') {
                    stack.push(path);
                }
            } else if is_shell_script(&path)
                && let Ok(rel) = path.strip_prefix(base)
            {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    Ok(out)
}

fn is_shell_script(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && matches!(ext, "sh" | "bash" | "zsh" | "ksh")
    {
        return true;
    }
    if let Ok(f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 64];
        let mut reader = std::io::BufReader::new(f);
        if let Ok(n) = reader.read(&mut buf) {
            let head = String::from_utf8_lossy(&buf[..n]);
            if head.starts_with("#!")
                && (head.contains("/sh")
                    || head.contains("/bash")
                    || head.contains("/zsh")
                    || head.contains("/ksh")
                    || head.contains("env sh")
                    || head.contains("env bash"))
            {
                return true;
            }
        }
    }
    false
}
