use std::path::Path;
use std::process::Command;

use crate::tools::output_util;

pub(super) const MAX_OUTPUT_LINES: usize = 800;

pub(super) fn parse_args(args_json: &str) -> Result<serde_json::Value, String> {
    crate::tools::parse_args_json(args_json)
}

pub(super) fn extract_safe_path(v: &serde_json::Value) -> Result<Option<String>, String> {
    match v.get("path").and_then(|x| x.as_str()) {
        Some(p) => {
            if !is_safe_rel_path(p) {
                Err("错误：path 必须是相对路径，且不能包含 \"..\" 或绝对路径".to_string())
            } else {
                Ok(Some(p.trim().to_string()))
            }
        }
        None => Ok(None),
    }
}

pub(super) fn require_safe_path(v: &serde_json::Value) -> Result<String, String> {
    match v.get("path").and_then(|x| x.as_str()) {
        Some(p) if is_safe_rel_path(p) => Ok(p.trim().to_string()),
        _ => Err("错误：缺少合法 path 参数".to_string()),
    }
}

pub(super) fn require_confirm(v: &serde_json::Value, tool_name: &str) -> Result<(), String> {
    if v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false) {
        Ok(())
    } else {
        Err(format!("拒绝执行：{} 需要 confirm=true", tool_name))
    }
}

pub(super) fn require_string_field<'a>(
    v: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, String> {
    match v.get(field).and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(format!("错误：缺少 {} 参数", field)),
    }
}

/// 统一处理 working/staged/all 三模式的 diff 类命令。
/// `extra_args` 为模式无关的附加参数（如 `--stat`、`--name-only`），
/// `context_fmt` 为可选 `-U{n}` 格式化字符串。
pub(super) fn run_diff_mode(
    v: &serde_json::Value,
    max_output_len: usize,
    working_dir: &Path,
    extra_args: &[&str],
    context_fmt: Option<String>,
    title_base: &str,
) -> String {
    let mode = v
        .get("mode")
        .and_then(|x| x.as_str())
        .unwrap_or("working")
        .trim()
        .to_lowercase();
    let path = match extract_safe_path(v) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let build_cmd = |staged: bool| {
        let mut cmd = Command::new("git");
        cmd.arg("diff");
        for a in extra_args {
            cmd.arg(*a);
        }
        if staged {
            cmd.arg("--staged");
        }
        if let Some(ref ctx) = context_fmt {
            cmd.arg(ctx);
        }
        if let Some(ref p) = path {
            cmd.arg("--").arg(p);
        }
        cmd.current_dir(working_dir);
        cmd
    };

    let title_suffix = |staged: bool| {
        if staged {
            format!("{} --staged", title_base)
        } else {
            title_base.to_string()
        }
    };

    match mode.as_str() {
        "working" => run_and_format(build_cmd(false), max_output_len, &title_suffix(false)),
        "staged" => run_and_format(build_cmd(true), max_output_len, &title_suffix(true)),
        "all" => {
            let a = run_and_format(build_cmd(false), max_output_len, &title_suffix(false));
            let b = run_and_format(build_cmd(true), max_output_len, &title_suffix(true));
            format!("{}\n\n====================\n\n{}", a, b)
        }
        _ => "错误：mode 仅支持 working | staged | all".to_string(),
    }
}
pub fn ensure_git_repo(working_dir: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .current_dir(working_dir)
        .output()
        .map_err(|e| {
            let b = format!("无法执行 git 命令: {}", e);
            output_util::append_notfound_install_hint(b, &e, "git")
        })?;
    if !out.status.success() {
        return Err("错误：当前工作目录不在 Git 仓库中".to_string());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    if s.trim() != "true" {
        return Err("错误：当前工作目录不在 Git 仓库中".to_string());
    }
    Ok(())
}

pub(super) fn is_safe_rel_path(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty() && !p.starts_with('/') && !p.contains("..")
}

pub(super) fn section_failed(s: &str) -> bool {
    let first = s.lines().next().unwrap_or("");
    let Some(idx) = first.find("(exit=") else {
        return false;
    };
    let rest = &first[idx + "(exit=".len()..];
    let Some(end) = rest.find(')') else {
        return false;
    };
    rest[..end]
        .trim()
        .parse::<i32>()
        .map(|c| c != 0)
        .unwrap_or(false)
}

pub(super) fn run_and_format(cmd: Command, max_output_len: usize, title: &str) -> String {
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
        output_util::CommandSpawnErrorStyle::ExecuteFailed,
    )
}
