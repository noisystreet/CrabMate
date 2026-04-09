//! 本地 `pre-commit` 封装：与语言无关，依赖仓库根目录配置文件。

use std::path::Path;
use std::process::{Command, Stdio};

use super::output_util;

const MAX_OUTPUT_LINES: usize = 800;

fn has_precommit_config(root: &Path) -> bool {
    root.join(".pre-commit-config.yaml").is_file() || root.join(".pre-commit-config.yml").is_file()
}

/// 在工作区根运行 `pre-commit run`。
///
/// - 无 `hook`：检查**暂存**文件（与 CLI 默认一致）。
/// - `all_files: true`：追加 `--all-files`。
/// - `files`：非空时追加 `--files` + 若干相对路径（与 `all_files` 同时出现时以 `files` 为准，不传 `--all-files`）。
/// - `hook`：指定 hook id（仅允许字母数字、`.`、`_`、`-`）。
pub fn pre_commit_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !has_precommit_config(workspace_root) {
        return "pre-commit run: 跳过（未找到 .pre-commit-config.yaml / .pre-commit-config.yml）"
            .to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Some(h) = v.get("hook").and_then(|x| x.as_str()).map(str::trim)
        && !h.is_empty()
        && !is_safe_hook_id(h)
    {
        return "错误：hook 仅允许字母数字与 ._-，且须以字母或数字开头".to_string();
    }

    let files = match parse_files_array(&v) {
        Ok(f) => f,
        Err(e) => return e,
    };

    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = Command::new("pre-commit");
    cmd.arg("run").current_dir(&base);

    if let Some(h) = v.get("hook").and_then(|x| x.as_str()).map(str::trim)
        && !h.is_empty()
    {
        cmd.arg(h);
    }

    if v.get("verbose").and_then(|x| x.as_bool()).unwrap_or(false) {
        cmd.arg("--verbose");
    }

    let all_files = v
        .get("all_files")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if !files.is_empty() {
        cmd.arg("--files");
        for p in &files {
            cmd.arg(p);
        }
    } else if all_files {
        cmd.arg("--all-files");
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    output_util::run_command_output_formatted(
        cmd,
        "pre-commit run",
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::StderrElseStdout,
        output_util::CommandSpawnErrorStyle::CannotStartCommand,
    )
}

fn is_safe_hook_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn parse_files_array(v: &serde_json::Value) -> Result<Vec<String>, String> {
    let Some(raw) = v.get("files") else {
        return Ok(Vec::new());
    };
    let Some(arr) = raw.as_array() else {
        return Err("错误：files 须为字符串数组".to_string());
    };
    let mut out = Vec::new();
    for x in arr {
        let s = x
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "错误：files 项须为非空字符串".to_string())?;
        if s.starts_with('/') || s.contains("..") {
            return Err(format!(
                "错误：files 项须为工作区内相对路径且不含 ..：{}",
                s
            ));
        }
        out.push(s.to_string());
    }
    Ok(out)
}
