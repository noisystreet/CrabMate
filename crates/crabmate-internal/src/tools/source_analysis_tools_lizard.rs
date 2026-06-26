//! Lizard 圈复杂度：`push_lizard_cli_args` 与 `lizard_complexity`。

use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{MAX_PATHS, filter_existing, parse_rel_paths_from_slice};
use crate::tools::output_util;
use crate::tools::tool_param_types::{LizardComplexityArgs, LizardSortKind};

fn push_lizard_threshold(cmd: &mut Command, args: &LizardComplexityArgs) {
    if let Some(threshold) = args.threshold
        && threshold > 0
        && threshold <= 200
    {
        cmd.arg("-C").arg(threshold.to_string());
    }
}

fn push_lizard_language(cmd: &mut Command, args: &LizardComplexityArgs) -> Result<(), String> {
    let Some(lang) = args
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };
    if lang.len() > 40 || lang.chars().any(|c| !c.is_alphanumeric() && c != ',') {
        return Err(format!("错误：language 值非法：{lang}"));
    }
    cmd.arg("-l").arg(lang);
    Ok(())
}

fn push_lizard_sort(cmd: &mut Command, sort: LizardSortKind) {
    let s = match sort {
        LizardSortKind::CyclomaticComplexity => "cyclomatic_complexity",
        LizardSortKind::Length => "length",
        LizardSortKind::TokenCount => "token_count",
        LizardSortKind::ParameterCount => "parameter_count",
        LizardSortKind::Nloc => "nloc",
    };
    cmd.arg("--sort").arg(s);
}

fn push_lizard_exclude_patterns(cmd: &mut Command, args: &LizardComplexityArgs) {
    for ex in &args.exclude {
        let ex = ex.trim();
        if ex.is_empty() || ex.len() > 160 || ex.contains("..") {
            continue;
        }
        cmd.arg("-x").arg(format!("*/{ex}/*"));
    }
}

fn push_lizard_cli_args(
    cmd: &mut Command,
    args: &LizardComplexityArgs,
    paths: &[String],
) -> Result<(), String> {
    push_lizard_threshold(cmd, args);
    push_lizard_language(cmd, args)?;
    if let Some(sort) = args.sort {
        push_lizard_sort(cmd, sort);
    }
    if args.warnings_only {
        cmd.arg("-w");
    }
    push_lizard_exclude_patterns(cmd, args);
    for p in paths {
        cmd.arg(p);
    }
    Ok(())
}

fn configure_lizard_stdio(cmd: &mut Command, base: &Path) {
    cmd.current_dir(base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
}

fn run_lizard_subprocess_output(
    base: &Path,
    args: &LizardComplexityArgs,
    paths: &[String],
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("lizard");
    configure_lizard_stdio(&mut cmd, base);
    push_lizard_cli_args(&mut cmd, args, paths)?;
    match cmd.output() {
        Ok(o) => Ok(o),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut cmd_py = Command::new("python3");
            cmd_py.arg("-m").arg("lizard");
            configure_lizard_stdio(&mut cmd_py, base);
            push_lizard_cli_args(&mut cmd_py, args, paths)?;
            match cmd_py.output() {
                Ok(o) => Ok(o),
                Err(e2) => Err(format!(
                    "lizard: 未找到命令 `lizard`（{e}），且 `python3 -m lizard` 亦失败（{e2}）。\
请安装：`pip install lizard` 或 `pip install --user lizard`，将 `lizard` 所在目录加入 PATH（`pip install --user` 时常见为 ~/.local/bin）；\
验证：`lizard --version` 或 `python3 -m lizard --version`。"
                )),
            }
        }
        Err(e) => Err(format!(
            "lizard: 无法启动（{e}）。请确认已安装对应 CLI 且在 PATH 中。"
        )),
    }
}

pub fn lizard_complexity(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: LizardComplexityArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 lizard_complexity 形状不一致: {e}"),
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

    let output = match run_lizard_subprocess_output(&base, &args, &paths) {
        Ok(o) => o,
        Err(msg) => return msg,
    };
    let code = output.status.code().unwrap_or(-1);
    let body = output_util::merge_process_output(
        &output,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
    );
    output_util::format_exited_command_output(
        "lizard",
        code,
        &body,
        max_output_len,
        super::MAX_OUTPUT_LINES,
    )
}
