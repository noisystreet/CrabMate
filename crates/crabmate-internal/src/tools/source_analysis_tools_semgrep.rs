//! Semgrep `scan` 调用：参数校验与 CLI 装配。

use std::path::Path;
use std::process::Command;

use super::{MAX_PATHS, filter_existing, parse_rel_paths_from_slice, run_and_format};
use crate::tools::tool_param_types::SemgrepScanArgs;

pub fn semgrep_scan(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: SemgrepScanArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 semgrep_scan 形状不一致: {e}"),
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

    let mut cmd = Command::new("semgrep");
    cmd.arg("scan").arg("--no-git-ignore").current_dir(&base);

    if let Err(msg) = apply_semgrep_config_flag(&mut cmd, &args) {
        return msg;
    }
    if let Err(msg) = apply_semgrep_severity_flags(&mut cmd, args.severity.as_deref()) {
        return msg;
    }
    if let Err(msg) = apply_semgrep_lang_flag(&mut cmd, args.lang.as_deref()) {
        return msg;
    }

    if args.json {
        cmd.arg("--json");
    }

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "semgrep scan")
}

fn apply_semgrep_config_flag(cmd: &mut Command, args: &SemgrepScanArgs) -> Result<(), String> {
    let config = args
        .config
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    if config.len() > 256 || config.contains("..") || config.contains('\n') {
        return Err("错误：config 值过长或含非法字符".to_string());
    }
    cmd.arg("--config").arg(config);
    Ok(())
}

fn apply_semgrep_severity_flags(
    cmd: &mut Command,
    severity_raw: Option<&str>,
) -> Result<(), String> {
    let Some(sev) = severity_raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let original = sev.to_string();
    for part in sev.split(',') {
        let token = part.trim().to_uppercase();
        match token.as_str() {
            "ERROR" | "WARNING" | "INFO" => {
                cmd.arg("--severity").arg(&token);
            }
            _ => {
                return Err(format!(
                    "错误：severity 须为 ERROR/WARNING/INFO（逗号分隔），收到 {original}"
                ));
            }
        }
    }
    Ok(())
}

fn apply_semgrep_lang_flag(cmd: &mut Command, lang: Option<&str>) -> Result<(), String> {
    let Some(lang) = lang.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if lang.len() > 40
        || lang
            .chars()
            .any(|c| !c.is_alphanumeric() && c != ',' && c != '+')
    {
        return Err(format!("错误：lang 值非法：{lang}"));
    }
    cmd.arg("--lang").arg(lang);
    Ok(())
}
